//! Exact p-curves against the certified seam bound: plane sections of
//! cylinders, cones and conic-chain cylinders (and an affine image) give
//! G spans, conics and lines on planes Q and linear spans, and circles
//! about the axis (spindle tori and revolved conics included), rulings
//! and meridians of every form of revolution iso spans, each at lattice
//! levels 0 to 2, the curves reparametrised by Möbius maps and split at
//! random knots so that their spans cross the cells' lines; every seam
//! bound is the rounding, refined to eight units of the last bit of the
//! tiles' coordinates. Knot points are the inverse's, fixed iso values
//! bit for bit; the refusals.

use std::f64::consts::TAU;

use glam::DVec3;
use varde_decide::bezier::Dir;
use varde_decide::num::trig::{acos, atan2, cos, sin};
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::certify::seam_bound_alone;
use crate::geom::conic::Conic;
use crate::geom::curve::Curve3D;
use crate::geom::form::{
    AffineKey, ConeKey, ConicCylinderKey, CylinderKey, PlaneKey, RevolvedKey, SphereKey, TorusKey,
};
use crate::geom::lattice::{Lattice, Level};
use crate::geom::pcurve::Kind;
use crate::infra::budget::MAX_WORK;

/// The most units of the last bit of the tiles' coordinates a seam
/// bound of an exact p-curve may be: the rounding of the tiles, the
/// curve's control points, the p-curve's values and the certificate's
/// own arithmetic.
const ULPS: f64 = 64.0;

/// The larger of two values not NaN.
fn larger(a: f64, b: f64) -> f64 {
    if a > b { a } else { b }
}

fn v(a: DVec3) -> [f64; 3] {
    a.to_array()
}

fn dv(a: [f64; 3]) -> DVec3 {
    DVec3::from_array(a)
}

/// The affine control point `(x, w)`.
fn hp(x: DVec3, w: f64) -> [f64; 4] {
    [x.x, x.y, x.z, w]
}

/// The form a lattice is (an affine image's, its source's).
fn source(lat: &Lattice) -> &FormKey {
    match lat.key() {
        FormKey::Affine(k) => k.source(),
        k => k,
    }
}

/// Whether coordinate `d` of the lattice's form turns about its axis.
fn turns(lat: &Lattice, d: Dir) -> bool {
    match (source(lat), d) {
        (FormKey::Plane(_) | FormKey::ConicCylinder(_), Dir::U) => false,
        (_, Dir::U) => true,
        (FormKey::Torus(t), Dir::V) => t.major() > t.minor(),
        _ => false,
    }
}

/// Points of the curve: eight per span and its last knot.
fn samples(c: &Curve3D) -> Vec<[f64; 3]> {
    let k = c.knots();
    let mut out = Vec::new();
    for w in k.windows(2) {
        for j in 0..8 {
            out.push(c.point_at(w[0] + (w[1] - w[0]) * j as f64 / 8.0).unwrap());
        }
    }
    out.push(c.point_at(k[k.len() - 1]).unwrap());
    out
}

/// The surface of `lat` `l` levels below its canonical rectangle's,
/// over the tiles holding the curve's points with one to spare about
/// each, within the canonical cells along a coordinate that doesn't
/// turn; capped.
fn surface_for(lat: &Lattice, l: u32, c: &Curve3D) -> Surface {
    let half = lat.half(0).unwrap();
    let hl = half.level;
    let xs = samples(c);
    // A provisional surface about the curve's middle around the axis.
    let mut prov = half.clone();
    if turns(lat, Dir::U) {
        let probe = Surface::new(lat.clone(), half.clone()).unwrap();
        let mid = probe.inverse(xs[xs.len() / 2]).unwrap().uv[0] * pow2(-(hl.u as i32));
        let first = mid.floor() as i64 - 1;
        prov.u = (first << hl.u)..((first + 3) << hl.u);
    }
    let prov = Surface::new(lat.clone(), prov).unwrap();
    let base = |x: [f64; 3]| {
        let uv = prov.inverse(x).unwrap().uv;
        [uv[0] * pow2(-(hl.u as i32)), uv[1] * pow2(-(hl.v as i32))]
    };
    let level = Level {
        u: hl.u + l,
        v: hl.v + l,
    };
    let ranges: Vec<std::ops::Range<i64>> = [
        (Dir::U, level.u, &half.u, hl.u),
        (Dir::V, level.v, &half.v, hl.v),
    ]
    .into_iter()
    .map(|(d, lv, canon, hlv)| {
        let k = ix(d);
        let mut vals: Vec<f64> = xs.iter().map(|&x| base(x)[k]).collect();
        if turns(lat, d) {
            // Continuous along the curve.
            for i in 1..vals.len() {
                vals[i] += 4.0 * ((vals[i - 1] - vals[i]) / 4.0 + 0.5).floor();
            }
        }
        let lo = vals
            .iter()
            .fold(f64::INFINITY, |m, &x| if x < m { x } else { m });
        let hi = vals
            .iter()
            .fold(f64::NEG_INFINITY, |m, &x| if x > m { x } else { m });
        let s = pow2(lv as i32);
        let (mut a, mut b) = ((lo * s).floor() as i64 - 1, (hi * s).floor() as i64 + 2);
        if turns(lat, d) {
            while b - a >= 4 << lv {
                a += 1;
                b -= 1;
            }
        } else {
            let sh = lv - hlv;
            a = a.max(canon.start << sh);
            b = b.min(canon.end << sh);
        }
        a..b
    })
    .collect();
    let rect = Rect {
        level,
        u: ranges[0].clone(),
        v: ranges[1].clone(),
    };
    let rect = lat.capped(&rect).unwrap();
    Surface::new(lat.clone(), rect).unwrap()
}

/// The largest coordinate magnitude of the tiles holding `p`'s spans.
fn scale(s: &Surface, p: &PCurve) -> f64 {
    let rect = s.rect();
    let mut m: f64 = 0.0;
    for span in p.spans() {
        let [lo, hi] = span.hull();
        let pick = |k: usize, r: &std::ops::Range<i64>| {
            let c = lo[k].floor() as i64;
            [c, c - 1]
                .into_iter()
                .find(|&i| r.contains(&i) && (i as f64) <= lo[k] && hi[k] <= (i + 1) as f64)
                .unwrap()
        };
        let t = s.tile(pick(0, &rect.u), pick(1, &rect.v)).unwrap();
        for h in t.net() {
            for x in &h[..3] {
                if x.abs() > m {
                    m = x.abs();
                }
            }
        }
    }
    m
}

/// Whether `x` is a point of a row of `s` along `u` on which every
/// control point is the same: a pole or apex.
fn axis_point(s: &Surface, x: [f64; 3]) -> bool {
    let r = s.rect();
    (r.v.start..=r.v.end).any(|j| {
        (r.u.start..r.u.end).any(|i| {
            s.row(Dir::U, j, i)
                .is_some_and(|row| row.iter().all(|p| p[..3] == x[..]))
        })
    })
}

/// Asserts that `p` is an exact p-curve of `c` on `s`: in tiles, the
/// curve's knots among its own over the same range, the knot points the
/// inverse's where the coordinate isn't `fixed`, and the seam bound
/// within [`ULPS`] of the rounding; that many units.
fn check(s: &Surface, c: &Curve3D, p: &PCurve, fixed: Option<(Dir, f64)>, what: &str) -> f64 {
    assert!(p.in_tiles(), "{what}");
    let (ck, pk) = (c.knots(), p.knots());
    assert_eq!(ck[0], pk[0], "{what}");
    assert_eq!(ck[ck.len() - 1], pk[pk.len() - 1], "{what}");
    for (k, t) in ck.iter().enumerate() {
        let j = pk.iter().position(|x| x == t).expect(what);
        let uv = p.knot_point(j).unwrap();
        let inv = s.inverse(c.knot_point(k).unwrap()).unwrap().uv;
        for d in [Dir::U, Dir::V] {
            let want = match fixed {
                Some((f, at)) if f == d => at,
                // A pole vertex: on its row.
                _ if axis_point(s, c.knot_point(k).unwrap()) => inv[ix(d)].round(),
                _ => inv[ix(d)],
            };
            assert_eq!(
                uv[ix(d)].to_bits(),
                want.to_bits(),
                "{what}: knot {k} {d:?}"
            );
        }
    }
    if let Some((f, at)) = fixed {
        for span in p.spans() {
            for x in span.coord(f).values() {
                assert_eq!(x[0].to_bits(), at.to_bits(), "{what}: fixed {f:?}");
            }
        }
    }
    let unit = scale(s, p) * f64::EPSILON;
    let b = seam_bound_alone(s, c, p, ULPS / 8.0 * unit, MAX_WORK).expect(what);
    let ulps = b.bound / unit;
    assert!(ulps <= ULPS, "{what}: {b:?}, {ulps} units");
    ulps
}

/// The ellipse `c + cos θ·a + sin θ·b` over its cells `from..to` at
/// level `l`.
fn ellipse(c: DVec3, a: DVec3, b: DVec3, l: u32, from: i64, to: i64) -> Curve3D {
    let conic = Conic::ellipse(v(c), [v(a), v(b)], [1.0, 1.0]).unwrap();
    Curve3D::ellipse(&conic, l, from, to).unwrap()
}

/// The curve with each span reparametrised by a random Möbius map of
/// ratio `λ` (weights `w₀`, `λ·w₁`, `λ²·w₂`) and split at a random `t`.
fn moved(rng: &mut Rng, c: &Curve3D) -> Curve3D {
    let lambda = rng.range(0.5, 2.0);
    let spans = c
        .spans()
        .iter()
        .map(|s| {
            let mut s = *s;
            s[1][3] *= lambda;
            s[2][3] *= lambda * lambda;
            s
        })
        .collect();
    let c = Curve3D::new(spans, c.knots().to_vec()).unwrap();
    let k = c.knots();
    let t = rng.range(k[0], k[k.len() - 1]);
    c.split(&[t]).unwrap()
}

/// An arc of `n` of a level-`l` ellipse's cells from a random one, its
/// spans moved by a random Möbius map and split.
fn arc(rng: &mut Rng, c: DVec3, a: DVec3, b: DVec3, n: i64) -> Curve3D {
    let l = (rng.next_u64() % 2) as u32;
    let from = (rng.next_u64() % (4 << l)) as i64;
    let e = ellipse(c, a, b, l, from, from + (n << l));
    moved(rng, &e)
}

/// A unit direction across the lattice's axis at a random angle, and the
/// one a quarter turn on.
fn across(rng: &mut Rng, lat: &Lattice) -> (DVec3, DVec3) {
    let [e1, e2, _] = lat.axes().map(dv);
    let psi = rng.range(0.0, TAU);
    let d = (cos(psi) * e1 + sin(psi) * e2).normalize();
    (d, dv(lat.axes()[2]).cross(d).normalize())
}

fn kinds(p: &PCurve) -> Vec<Kind> {
    p.spans().iter().map(PSpan::kind).collect()
}

/// A cylinder's section by an oblique plane through its axis: the
/// ellipse over half a turn.
fn cylinder_section(rng: &mut Rng, lat: &Lattice, r: f64) -> Curve3D {
    let (o, a) = (dv(lat.origin()), dv(lat.axes()[2]));
    let (d, m) = across(rng, lat);
    let g = rng.range(0.2, 1.3);
    let major = (sin(g) * a - cos(g) * d) * (r / cos(g));
    let centre = o + rng.range(-50.0, 50.0) * a;
    arc(rng, centre, major, m * r, 2)
}

/// A cone's section by a plane through a point of its axis, tilted
/// towards a random direction across it, cutting an ellipse.
fn cone_section(rng: &mut Rng, lat: &Lattice, ca: f64, sa: f64) -> Curve3D {
    let (apex, a) = (dv(lat.origin()), dv(lat.axes()[2]));
    let (e, m) = across(rng, lat);
    let h = rng.range(1.0, 100.0);
    // `tan γ·tan α` below 1: an ellipse.
    let g = atan2(rng.range(0.1, 0.8) * ca, sa);
    let (along, out) = (ca * a + sa * e, ca * a - sa * e);
    let p0 = apex + h * cos(g) / cos(g + acos(ca)) * along;
    let p1 = apex + h * cos(g) / cos(g - acos(ca)) * out;
    let c = (p0 + p1) / 2.0;
    let tan = sa / ca;
    let (za, ze) = ((c - apex).dot(a), (c - apex).dot(e));
    let b = (tan * tan * za * za - ze * ze).sqrt();
    // A quarter of the ellipse: its angle about the apex stays below a
    // half turn however eccentric it is.
    arc(rng, c, (p0 - p1) / 2.0, b * m, 1)
}

#[test]
fn plane_sections_of_cylinders_and_cones_are_g_spans() {
    let mut worst: f64 = 0.0;
    // How many p-curves the cells' lines cut, of how many.
    let (mut cut, mut total) = (0, 0);
    for seed in varde_testing::seeds(4, 40) {
        let mut rng = Rng::new(seed);
        let (p, axis) = (rng.point(100.0).to_array(), rng.direction().to_array());
        let r = rng.log_range(0.5, 200.0);
        let cyl = Lattice::new(CylinderKey::new(p, axis, r).unwrap().into()).unwrap();
        let c = cylinder_section(&mut rng, &cyl, r);
        let (along, out) = (rng.range(0.2, 1.0), rng.range(0.2, 1.0));
        let key = ConeKey::new(p, axis, along, out).unwrap();
        let (ca, sa) = (key.cos(), key.sin());
        let cone = Lattice::new(key.into()).unwrap();
        let k = cone_section(&mut rng, &cone, ca, sa);
        for (lat, c, name) in [(&cyl, &c, "cylinder"), (&cone, &k, "cone")] {
            for l in 0..3 {
                let s = surface_for(lat, l, c);
                let what = format!("seed {seed}, {name}, level {l}");
                let pc = trace(&s, c).expect(&what);
                let ks = kinds(&pc);
                assert!(
                    ks.iter().all(|k| matches!(k, Kind::G | Kind::Iso)),
                    "{what}: {ks:?}"
                );
                assert!(ks.contains(&Kind::G), "{what}: {ks:?}");
                if pc.spans().len() > c.spans().len() {
                    cut += 1;
                }
                total += 1;
                worst = larger(worst, check(&s, c, &pc, None, &what));
            }
        }
    }
    assert!(2 * cut > total, "{cut} of {total} cut");
    println!("worst: {worst} units of the last bit");
}

#[test]
fn plane_sections_of_conic_chain_cylinders_cut_at_the_joints() {
    // The profile's three quarters of an ellipse, the section's ellipse
    // the same one lifted, parametrised from 45° on: its spans cross the
    // profile's joints.
    let mut worst: f64 = 0.0;
    for seed in varde_testing::seeds(4, 40) {
        let mut rng = Rng::new(seed);
        let a = rng.direction();
        let (p, q) = a.any_orthonormal_pair();
        let r = rng.log_range(1.0, 100.0);
        let (alpha, beta) = (r * p, 0.6 * r * q);
        let centre = rng.point(100.0);
        let profile = Conic::ellipse(v(centre), [v(alpha), v(beta)], [1.0, 1.0]).unwrap();
        let spans: Vec<_> = (0..3).map(|k| profile.quarter::<4>(k).unwrap()).collect();
        let lat = Lattice::new(ConicCylinderKey::new(v(a), &spans).unwrap().into()).unwrap();
        let a = dv(lat.axes()[2]);
        // The plane `n·x = d`, `n` tilted from the axis.
        let n = (a + rng.range(-0.8, 0.8) * p + rng.range(-0.8, 0.8) * q).normalize();
        let d = rng.range(-50.0, 50.0);
        let lift = |x: DVec3| x - x.dot(a) * a + (d - (x - x.dot(a) * a).dot(n)) / n.dot(a) * a;
        let vec = |x: DVec3| x - x.dot(a) * a - (x - x.dot(a) * a).dot(n) / n.dot(a) * a;
        let s2 = std::f64::consts::FRAC_1_SQRT_2;
        let (a1, b1) = ((alpha + beta) * s2, (beta - alpha) * s2);
        let e = ellipse(lift(centre), vec(a1), vec(b1), 0, 0, 2);
        let c = moved(&mut rng, &e);
        for l in 0..3 {
            let s = surface_for(&lat, l, &c);
            let what = format!("seed {seed}, level {l}");
            let pc = trace(&s, &c).expect(&what);
            let ks = kinds(&pc);
            assert!(
                ks.iter().all(|k| matches!(k, Kind::G | Kind::Iso)),
                "{what}: {ks:?}"
            );
            // Two joints inside the curve's spans, at a knot each whose
            // `u` is the joint's, exactly.
            let lines = pc
                .knots()
                .iter()
                .enumerate()
                .filter(|&(j, t)| {
                    !c.knots().contains(t) && {
                        let u = pc.knot_point(j).unwrap()[0] * pow2(-(s.rect().level.u as i32));
                        u == u.round()
                    }
                })
                .count();
            assert!(lines >= 2, "{what}: {lines}");
            worst = larger(worst, check(&s, &c, &pc, None, &what));
        }
    }
    println!("worst: {worst} units of the last bit");
}

#[test]
fn affine_images_keep_their_g_spans() {
    let mut worst: f64 = 0.0;
    for seed in varde_testing::seeds(3, 20) {
        let mut rng = Rng::new(seed);
        let (p, axis) = (rng.point(100.0).to_array(), rng.direction().to_array());
        let r = rng.log_range(0.5, 50.0);
        let key = CylinderKey::new(p, axis, r).unwrap();
        let src = Lattice::new(key.into()).unwrap();
        let c = cylinder_section(&mut rng, &src, r);
        let m: [[f64; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|j| rng.range(-0.5, 0.5) + if i == j { 1.5 } else { 0.0 })
        });
        let shift = rng.point(100.0).to_array();
        let map = |x: [f64; 3]| -> [f64; 3] {
            std::array::from_fn(|i| m[i][0] * x[0] + m[i][1] * x[1] + m[i][2] * x[2] + shift[i])
        };
        // The image's spans: control points mapped, weights kept.
        let spans = c
            .spans()
            .iter()
            .map(|s| {
                s.map(|h| {
                    let x = map([h[0], h[1], h[2]]);
                    [x[0], x[1], x[2], h[3]]
                })
            })
            .collect();
        let c = Curve3D::new(spans, c.knots().to_vec()).unwrap();
        let lat = Lattice::new(AffineKey::new(key.into(), m, shift).unwrap().into()).unwrap();
        for l in 0..2 {
            let s = surface_for(&lat, l, &c);
            let what = format!("seed {seed}, level {l}");
            let pc = trace(&s, &c).expect(&what);
            assert!(kinds(&pc).contains(&Kind::G), "{what}");
            worst = larger(worst, check(&s, &c, &pc, None, &what));
        }
    }
    println!("worst: {worst} units of the last bit");
}

#[test]
fn curves_on_planes_are_q_and_linear_spans() {
    let mut worst: f64 = 0.0;
    for seed in varde_testing::seeds(4, 40) {
        let mut rng = Rng::new(seed);
        let (n, p) = (rng.direction().to_array(), rng.point(100.0).to_array());
        let lat = Lattice::new(PlaneKey::new(n, p).unwrap().into()).unwrap();
        let [u, w, _] = lat.axes().map(dv);
        let o = dv(lat.origin());
        let at = |a: f64, b: f64| o + a * u + b * w;
        let r = rng.log_range(1.0, 100.0);
        let c0 = at(rng.range(-100.0, 100.0), rng.range(-100.0, 100.0));
        let psi = rng.range(0.0, TAU);
        let (a, b) = (cos(psi) * u + sin(psi) * w, cos(psi) * w - sin(psi) * u);
        let e = arc(&mut rng, c0, r * a, 0.5 * r * b, 3);
        let para = Conic::parabola(v(c0), [v(a), v(b)], r).unwrap();
        let para = Curve3D::conic_arcs(&para, &[-1.0, 0.5, 2.0]).unwrap();
        for (c, name) in [(e, "ellipse"), (para, "parabola")] {
            for l in 0..3 {
                let s = surface_for(&lat, l, &c);
                let what = format!("seed {seed}, {name}, level {l}");
                let pc = trace(&s, &c).expect(&what);
                assert!(kinds(&pc).iter().all(|k| *k == Kind::Q), "{what}");
                worst = larger(worst, check(&s, &c, &pc, None, &what));
            }
        }
    }
    // Lines at a Möbius speed on a world plane, their middle points the
    // exact weighted means: linear spans.
    for seed in varde_testing::seeds(4, 40) {
        let mut rng = Rng::new(seed);
        let z = rng.range(-100.0, 100.0);
        let lat = Lattice::new(
            PlaneKey::new([0.0, 0.0, 1.0], [0.0, 0.0, z])
                .unwrap()
                .into(),
        )
        .unwrap();
        let grid = |rng: &mut Rng| (rng.range(-100.0, 100.0) * 1024.0).round() / 1024.0;
        let (x0, y0, x1, y1) = (
            grid(&mut rng),
            grid(&mut rng),
            grid(&mut rng),
            grid(&mut rng),
        );
        // Weights 1 and 3: the middle point `(p₀ + 3p₂)/4`, weight 2.
        let mid = |a: f64, b: f64| (a + 3.0 * b) / 4.0;
        let c = Curve3D::new(
            vec![[
                [x0, y0, z, 1.0],
                [mid(x0, x1), mid(y0, y1), z, 2.0],
                [x1, y1, z, 3.0],
            ]],
            vec![0.0, 1.0],
        )
        .unwrap();
        for l in 0..3 {
            let s = surface_for(&lat, l, &c);
            let what = format!("seed {seed}, line, level {l}");
            let pc = trace(&s, &c).expect(&what);
            assert!(kinds(&pc).iter().all(|k| *k == Kind::Linear), "{what}");
            worst = larger(worst, check(&s, &c, &pc, None, &what));
        }
    }
    println!("worst: {worst} units of the last bit");
}

/// A circle about the axis `a` through `centre`, radius `r`, over
/// `n` quarter turns, in a frame turned at random about the axis.
fn about(rng: &mut Rng, lat: &Lattice, centre: DVec3, r: f64, n: i64) -> Curve3D {
    let (d, m) = across(rng, lat);
    arc(rng, centre, r * d, r * m, n)
}

/// A meridian circle in the half-plane of `d`: centre `centre`, radius
/// `r`, from `start` radians towards the axis `a` over `n` quarter turns
/// of level-1 cells (eighth turns).
fn meridian(
    rng: &mut Rng,
    centre: DVec3,
    d: DVec3,
    a: DVec3,
    r: f64,
    start: f64,
    n: i64,
) -> Curve3D {
    let (c, s) = (cos(start), sin(start));
    let e = ellipse(centre, r * (c * d + s * a), r * (c * a - s * d), 1, 0, n);
    moved(rng, &e)
}

/// Asserts the iso p-curve of `c` on the line `fixed` through its first
/// point: iso spans (G with a constant Möbius coordinate where `quad`)
/// within the rounding.
fn iso_check(lat: &Lattice, c: &Curve3D, fixed: Dir, quad: bool, what: &str) -> f64 {
    let mut worst: f64 = 0.0;
    for l in 0..3 {
        let s = surface_for(lat, l, c);
        let at = s.inverse(c.knot_point(0).unwrap()).unwrap().uv[ix(fixed)];
        let what = format!("{what}, level {l}");
        let pc = iso(&s, c, fixed, at).expect(&what);
        for k in kinds(&pc) {
            assert!(k == Kind::Iso || (quad && k == Kind::G), "{what}: {k:?}");
        }
        worst = larger(worst, check(&s, c, &pc, Some((fixed, at)), &what));
    }
    worst
}

#[test]
fn circles_about_the_axis_and_rulings_are_isos() {
    let mut worst: f64 = 0.0;
    for seed in varde_testing::seeds(4, 40) {
        let mut rng = Rng::new(seed);
        let (p, axis) = (rng.point(100.0).to_array(), rng.direction().to_array());
        let r = rng.log_range(0.5, 200.0);
        let what = |f: &str| format!("seed {seed}, {f}");
        // Cylinder: a circle, a ruling at a quadratic speed.
        let lat = Lattice::new(CylinderKey::new(p, axis, r).unwrap().into()).unwrap();
        let (o, a) = (dv(lat.origin()), dv(lat.axes()[2]));
        let h = rng.range(-50.0, 50.0);
        let c = about(&mut rng, &lat, o + h * a, r, 2);
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::V, false, &what("cylinder circle")),
        );
        let (d, _) = across(&mut rng, &lat);
        let foot = o + r * d;
        let (z0, z1) = (rng.range(-50.0, 0.0), rng.range(1.0, 50.0));
        let ruling = Curve3D::new(
            vec![[
                hp(foot + z0 * a, 1.0),
                hp(foot + (0.3 * z0 + 0.7 * z1) * a, 0.5),
                hp(foot + z1 * a, 1.0),
            ]],
            vec![0.0, 1.0],
        )
        .unwrap();
        worst = larger(
            worst,
            iso_check(&lat, &ruling, Dir::U, true, &what("cylinder ruling")),
        );
        // Cone: a circle at a slant distance.
        let (along, out) = (rng.range(0.2, 1.0), rng.range(0.2, 1.0));
        let key = ConeKey::new(p, axis, along, out).unwrap();
        let (ca, sa) = (key.cos(), key.sin());
        let lat = Lattice::new(key.into()).unwrap();
        let (apex, a) = (dv(lat.origin()), dv(lat.axes()[2]));
        let t = rng.range(1.0, 100.0);
        let c = about(&mut rng, &lat, apex + t * ca * a, t * sa, 2);
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::V, false, &what("cone circle")),
        );
        // Sphere: a parallel.
        let lat = Lattice::new(SphereKey::new(p, r, axis).unwrap().into()).unwrap();
        let (o, a) = (dv(lat.origin()), dv(lat.axes()[2]));
        let phi = rng.range(-1.2, 1.2);
        let c = about(&mut rng, &lat, o + r * sin(phi) * a, r * cos(phi), 2);
        worst = larger(worst, iso_check(&lat, &c, Dir::V, false, &what("parallel")));
        // Ring torus: a circle about the axis at a tube angle.
        let big = r * rng.range(1.5, 3.0);
        let lat = Lattice::new(TorusKey::new(p, axis, big, r).unwrap().into()).unwrap();
        let (o, a) = (dv(lat.origin()), dv(lat.axes()[2]));
        let psi = rng.range(0.0, TAU);
        let c = about(&mut rng, &lat, o + r * sin(psi) * a, big + r * cos(psi), 2);
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::V, false, &what("torus circle")),
        );
        // Spindle torus: a circle about the axis on its outer part.
        let small = r * rng.range(0.2, 0.8);
        let lat = Lattice::new(TorusKey::new(p, axis, small, r).unwrap().into()).unwrap();
        let (o, a) = (dv(lat.origin()), dv(lat.axes()[2]));
        let psi = acos(-small / r) * rng.range(-0.8, 0.8);
        let c = about(
            &mut rng,
            &lat,
            o + r * sin(psi) * a,
            small + r * cos(psi),
            2,
        );
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::V, false, &what("spindle circle")),
        );
        // Revolved ellipse: the circle through its profile's first joint,
        // `(cr, cz + r/2)` from the given point along the given axis.
        let (cr, cz) = (r * rng.range(1.5, 3.0), rng.range(-50.0, 50.0));
        let prof = Conic::ellipse([cr, cz], [[1.0, 0.0], [0.0, 1.0]], [r, 0.5 * r]).unwrap();
        let spans: Vec<_> = (0..3).map(|k| prof.quarter::<3>(k).unwrap()).collect();
        let lat = Lattice::new(RevolvedKey::new(p, axis, &spans).unwrap().into()).unwrap();
        let a = dv(axis).normalize();
        let c = about(&mut rng, &lat, dv(p) + (cz + 0.5 * r) * a, cr, 2);
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::V, false, &what("revolved circle")),
        );
    }
    println!("worst: {worst} units of the last bit");
}

#[test]
fn meridians_are_isos_cut_at_their_cells() {
    let mut worst: f64 = 0.0;
    for seed in varde_testing::seeds(4, 40) {
        let mut rng = Rng::new(seed);
        let (p, axis) = (rng.point(100.0).to_array(), rng.direction().to_array());
        let r = rng.log_range(0.5, 200.0);
        let what = |f: &str| format!("seed {seed}, {f}");
        // Sphere: across the equator, and from it to a pole.
        let lat = Lattice::new(SphereKey::new(p, r, axis).unwrap().into()).unwrap();
        let (o, a) = (dv(lat.origin()), dv(lat.axes()[2]));
        let (d, _) = across(&mut rng, &lat);
        let start = rng.range(-1.4, -0.1);
        let c = meridian(&mut rng, o, d, a, r, start, 2);
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::U, false, &what("sphere meridian")),
        );
        // Its end the lattice's north pole, a vertex there.
        let c = meridian(&mut rng, o, d, a, r, 0.0, 2);
        let half = Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap();
        let pole = half.row(Dir::U, 1, 0).unwrap()[0];
        let mut spans = c.spans().to_vec();
        let n = spans.len() - 1;
        spans[n][2] = [pole[0], pole[1], pole[2], spans[n][2][3]];
        let c = Curve3D::new(spans, c.knots().to_vec()).unwrap();
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::U, false, &what("to the pole")),
        );
        // Ring torus: half the tube from a random angle, across its
        // equators and its top or bottom.
        let big = r * rng.range(1.5, 3.0);
        let lat = Lattice::new(TorusKey::new(p, axis, big, r).unwrap().into()).unwrap();
        let (o, a) = (dv(lat.origin()), dv(lat.axes()[2]));
        let (d, _) = across(&mut rng, &lat);
        let start = rng.range(0.0, TAU);
        let c = meridian(&mut rng, o + big * d, d, a, r, start, 4);
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::U, false, &what("ring meridian")),
        );
        // Spindle torus: its outer part across the outer equator.
        let small = r * rng.range(0.2, 0.8);
        let lat = Lattice::new(TorusKey::new(p, axis, small, r).unwrap().into()).unwrap();
        let (o, a) = (dv(lat.origin()), dv(lat.axes()[2]));
        let (d, _) = across(&mut rng, &lat);
        let start = rng.range(-1.4, -0.1);
        let c = meridian(&mut rng, o + small * d, d, a, r, start, 2);
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::U, false, &what("spindle meridian")),
        );
        // Revolved ellipse: its profile's three quarters, the meridian the
        // same ellipse parametrised from 45° on, across two joints.
        let rr = r * rng.range(0.3, 1.0);
        let cz = rng.range(-50.0, 50.0);
        let (cr, axes) = (r * rng.range(1.5, 3.0), [[1.0, 0.0], [0.0, 1.0]]);
        let prof = Conic::ellipse([cr, cz], axes, [r, rr]).unwrap();
        let spans: Vec<_> = (0..3).map(|k| prof.quarter::<3>(k).unwrap()).collect();
        let lat = Lattice::new(RevolvedKey::new(p, axis, &spans).unwrap().into()).unwrap();
        // The profile's `(ρ, z)` from the given point along the given axis.
        let (o, a) = (dv(p), dv(axis).normalize());
        let (d, _) = across(&mut rng, &lat);
        let s2 = std::f64::consts::FRAC_1_SQRT_2;
        let (al, be) = (r * d, rr * a);
        let e = ellipse(o + cr * d + cz * a, (al + be) * s2, (be - al) * s2, 0, 0, 2);
        let c = moved(&mut rng, &e);
        worst = larger(
            worst,
            iso_check(&lat, &c, Dir::U, false, &what("revolved meridian")),
        );
    }
    println!("worst: {worst} units of the last bit");
}

#[test]
fn refusals() {
    let mut rng = Rng::new(7);
    let (p, axis) = ([1.0, 2.0, 3.0], rng.direction().to_array());
    let lat = Lattice::new(SphereKey::new(p, 10.0, axis).unwrap().into()).unwrap();
    let (o, a) = (dv(lat.origin()), dv(lat.axes()[2]));
    let (d, _) = across(&mut rng, &lat);
    // Over the north pole: from 45° to 135° of latitude.
    let over = meridian(&mut rng, o, d, a, 10.0, 0.25 * std::f64::consts::PI, 4);
    let s = Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap();
    assert_eq!(trace(&s, &over), Err(ExactError::Form));
    assert_eq!(iso(&s, &over, Dir::U, f64::NAN), Err(ExactError::At));
    assert!(matches!(
        iso(&s, &over, Dir::U, 0.5),
        Err(ExactError::Axis { .. })
    ));
    // A cylinder's section outside a rectangle of one tile.
    let lat = Lattice::new(CylinderKey::new(p, axis, 10.0).unwrap().into()).unwrap();
    let c = cylinder_section(&mut rng, &lat, 10.0);
    let rect = Rect {
        level: Level { u: 2, v: 2 },
        u: 0..1,
        v: 2..3,
    };
    let s = Surface::new(lat, rect).unwrap();
    let got = trace(&s, &c);
    assert!(matches!(got, Err(ExactError::Domain { .. })), "{got:?}");
}
