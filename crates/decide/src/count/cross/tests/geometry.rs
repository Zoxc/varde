//! The conventions on geometry: every term computed directly from
//! generic random geometry (no ties, configurations near one skipped),
//! the identities against the crossings counted on the face itself,
//! `−sign(C′·n_f)` each. Faces: star-shaped polygons in random planes (both
//! facings), and a cylinder band `(x, R sin θ, R cos θ)` over a
//! rectangle in `(x, θ)` reaching past `θ = −π/2`, so it folds under
//! `UP` and its down-facing part counts against its up-facing part; its
//! `θ` sides are circular arcs whose shadows run back over themselves.
//! Edges are segments; a face's trims are stored either way round. Only
//! the stated convention matches; for `x21` each of the three other
//! combinations (`s20` as an upward ray, `+Σ s11`, both) mismatches.

use std::f64::consts::PI;

use glam::DVec3;
use varde_testkit::rng::Rng;

use super::super::*;
use super::pick;

/// Closer than this to a tie, a trial is skipped.
const GAP: f64 = 1e-7;

/// A curve along its stored direction over `u ∈ [0, 1]`.
#[derive(Clone, Copy, Debug)]
enum Curve {
    Line(DVec3, DVec3),
    /// `(x, r sin θ, r cos θ)`, `θ` from `th[0]` to `th[1]`.
    Arc {
        x: f64,
        r: f64,
        th: [f64; 2],
    },
}

impl Curve {
    fn at(&self, u: f64) -> DVec3 {
        match *self {
            Curve::Line(a, b) => a + (b - a) * u,
            Curve::Arc { x, r, th } => {
                let t = th[0] + (th[1] - th[0]) * u;
                DVec3::new(x, r * libm::sin(t), r * libm::cos(t))
            }
        }
    }

    fn tangent(&self, u: f64) -> DVec3 {
        match *self {
            Curve::Line(a, b) => b - a,
            Curve::Arc { r, th, .. } => {
                let t = th[0] + (th[1] - th[0]) * u;
                DVec3::new(0.0, r * libm::cos(t), -r * libm::sin(t)) * (th[1] - th[0])
            }
        }
    }

    fn reversed(&self) -> Curve {
        match *self {
            Curve::Line(a, b) => Curve::Line(b, a),
            Curve::Arc { x, r, th } => Curve::Arc {
                x,
                r,
                th: [th[1], th[0]],
            },
        }
    }
}

fn cross_z(p: DVec3, q: DVec3) -> f64 {
    p.x * q.y - p.y * q.x
}

/// Parameters `(u_line, u_other)` where the shadows of a segment and a
/// curve cross; `None` near a tie.
fn shadow_crossings(line: (DVec3, DVec3), other: &Curve) -> Option<Vec<(f64, f64)>> {
    let (a, b) = line;
    let d = b - a;
    let mut out = Vec::new();
    match *other {
        Curve::Line(p, q) => {
            let e = q - p;
            let den = cross_z(d, e);
            if den.abs() < GAP {
                return None;
            }
            let t = cross_z(p - a, e) / den;
            let s = cross_z(p - a, d) / den;
            if near_end(t) || near_end(s) {
                return None;
            }
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&s) {
                out.push((t, s));
            }
        }
        Curve::Arc { x, r, th } => {
            if d.x.abs() < GAP {
                return None;
            }
            let t = (x - a.x) / d.x;
            if near_end(t) {
                return None;
            }
            if !(0.0..=1.0).contains(&t) {
                return Some(out);
            }
            let s = (a.y + t * d.y) / r;
            if (s.abs() - 1.0).abs() < GAP {
                return None;
            }
            if s.abs() > 1.0 {
                return Some(out);
            }
            let base = libm::asin(s);
            for c in [base, PI - base] {
                for k in [-2.0, 0.0, 2.0] {
                    let u = (c + k * PI - th[0]) / (th[1] - th[0]);
                    if near_end(u) {
                        return None;
                    }
                    if (0.0..=1.0).contains(&u) {
                        out.push((t, u));
                    }
                }
            }
        }
    }
    Some(out)
}

fn near_end(t: f64) -> bool {
    t.abs() < GAP || (t - 1.0).abs() < GAP
}

fn sign(v: f64) -> Option<i64> {
    if v.abs() < GAP {
        None
    } else {
        Some(if v > 0.0 { 1 } else { -1 })
    }
}

/// `s11(e, g)`: the crossings of the shadows with `g` above `e`, each of
/// the degree `sign(T_g × T_e)` (the winding of `C_e(t) − C_g(s)`).
/// One of the two is a segment.
fn s11(e: &Curve, g: &Curve) -> Option<i64> {
    let (seg, other, e_is_seg) = match (*e, *g) {
        (Curve::Line(a, b), _) => ((a, b), g, true),
        (_, Curve::Line(a, b)) => ((a, b), e, false),
        _ => unreachable!("one of the two is a segment"),
    };
    let mut s = 0;
    for (ul, uo) in shadow_crossings(seg, other)? {
        let (ue, ug) = if e_is_seg { (ul, uo) } else { (uo, ul) };
        let deg = sign(cross_z(g.tangent(ug), e.tangent(ue)))?;
        if sign(g.at(ug).z - e.at(ue).z)? > 0 {
            s += deg;
        }
    }
    Some(s)
}

/// A face: its layers over a point's column, the crossings of a segment
/// with it, and its loop.
enum Face {
    /// A star-shaped polygon in a plane, its normal by its loop.
    Polygon(Vec<DVec3>, DVec3),
    /// The cylinder band over `[x0, x1] × [θ0, θ1]`, outward when `out`.
    Band {
        r: f64,
        x: [f64; 2],
        th: [f64; 2],
        out: bool,
    },
}

impl Face {
    fn random(rng: &mut Rng) -> Face {
        if rng.next_u64().is_multiple_of(2) {
            let n = loop {
                let n = rng.direction();
                if n.z.abs() > 0.1 {
                    break n;
                }
            };
            let u = n.any_orthonormal_vector();
            let v = n.cross(u);
            let c = rng.point(0.5);
            let k = 3 + pick(rng, 4);
            // Every gap below π, so the polygon is star-shaped about `c`.
            let angles: Vec<f64> = (0..k)
                .map(|i| (i as f64 + rng.range(0.3, 0.7)) * 2.0 * PI / k as f64)
                .collect();
            Face::Polygon(
                angles
                    .iter()
                    .map(|&a| c + (u * libm::cos(a) + v * libm::sin(a)) * rng.range(0.6, 1.5))
                    .collect(),
                n,
            )
        } else {
            Face::Band {
                r: rng.range(0.8, 1.6),
                x: [rng.range(-1.2, -0.4), rng.range(0.4, 1.2)],
                th: [rng.range(-2.4, -1.7), rng.range(0.3, 1.4)],
                out: rng.next_u64().is_multiple_of(2),
            }
        }
    }

    /// The loop's curves in loop order.
    fn trims(&self) -> Vec<Curve> {
        match self {
            Face::Polygon(p, _) => (0..p.len())
                .map(|i| Curve::Line(p[i], p[(i + 1) % p.len()]))
                .collect(),
            &Face::Band { r, x, th, out } => {
                let p = |xx: f64, t: f64| DVec3::new(xx, r * libm::sin(t), r * libm::cos(t));
                // Counter-clockwise in (x, θ): outward.
                let ccw = vec![
                    Curve::Line(p(x[0], th[0]), p(x[1], th[0])),
                    Curve::Arc { x: x[1], r, th },
                    Curve::Line(p(x[1], th[1]), p(x[0], th[1])),
                    Curve::Arc {
                        x: x[0],
                        r,
                        th: [th[1], th[0]],
                    },
                ];
                if out {
                    ccw
                } else {
                    ccw.iter().rev().map(Curve::reversed).collect()
                }
            }
        }
    }

    fn normal(&self, p: DVec3) -> DVec3 {
        match *self {
            Face::Polygon(_, n) => n,
            Face::Band { out, .. } => {
                let n = DVec3::new(0.0, p.y, p.z);
                if out { n } else { -n }
            }
        }
    }

    /// Whether `p` on the face's surface lies inside its trim; `None`
    /// near the trim.
    fn inside(&self, p: DVec3) -> Option<bool> {
        match self {
            Face::Polygon(v, _) => {
                // Crossing number of the shadow's `+x` ray from `p`.
                let mut odd = false;
                for i in 0..v.len() {
                    let (a, b) = (v[i], v[(i + 1) % v.len()]);
                    let (sa, sb) = (sign(a.y - p.y)?, sign(b.y - p.y)?);
                    if sa != sb {
                        let x = a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y);
                        odd ^= sign(x - p.x)? > 0;
                    }
                }
                Some(odd)
            }
            &Face::Band { x, th, .. } => {
                let t = libm::atan2(p.y, p.z);
                for b in [p.x - x[0], x[1] - p.x, t - th[0], th[1] - t] {
                    sign(b)?;
                }
                Some(p.x > x[0] && p.x < x[1] && t > th[0] && t < th[1])
            }
        }
    }

    /// The points of the face over `q`'s column, with their facing
    /// (`sign(n_z)`).
    fn layers(&self, q: DVec3) -> Option<Vec<(f64, i64)>> {
        let mut pts = Vec::new();
        match self {
            Face::Polygon(v, _) => {
                let n = self.normal(q);
                let z = v[0].z - (n.x * (q.x - v[0].x) + n.y * (q.y - v[0].y)) / n.z;
                pts.push(DVec3::new(q.x, q.y, z));
            }
            &Face::Band { r, .. } => {
                let s = q.y / r;
                if (s.abs() - 1.0).abs() < GAP {
                    return None;
                }
                if s.abs() < 1.0 {
                    let c = libm::sqrt(1.0 - s * s) * r;
                    pts.push(DVec3::new(q.x, q.y, c));
                    pts.push(DVec3::new(q.x, q.y, -c));
                }
            }
        }
        let mut out = Vec::new();
        for p in pts {
            if self.inside(p)? {
                out.push((p.z, sign(self.normal(p).z)?));
            }
        }
        Some(out)
    }

    /// `s02(q, f)` (upward ray: `+1` per up-facing layer above) or, with
    /// `down`, `s20(q, f)` (`+1` per down-facing layer below).
    fn ray(&self, q: DVec3, down: bool) -> Option<i64> {
        let mut s = 0;
        for (z, facing) in self.layers(q)? {
            let above = sign(z - q.z)? > 0;
            s += match (down, above) {
                (false, true) => facing,
                (true, false) => -facing,
                _ => 0,
            };
        }
        Some(s)
    }

    /// The segment's crossings through the face, `−sign(d·n)` each.
    fn crossings(&self, a: DVec3, b: DVec3) -> Option<i64> {
        let d = b - a;
        let mut ts = Vec::new();
        match self {
            Face::Polygon(v, _) => {
                let n = self.normal(a);
                let den = d.dot(n);
                if den.abs() < GAP {
                    return None;
                }
                ts.push((v[0] - a).dot(n) / den);
            }
            &Face::Band { r, .. } => {
                let qa = d.y * d.y + d.z * d.z;
                let qb = 2.0 * (a.y * d.y + a.z * d.z);
                let qc = a.y * a.y + a.z * a.z - r * r;
                let disc = qb * qb - 4.0 * qa * qc;
                if disc.abs() < GAP {
                    return None;
                }
                if disc > 0.0 {
                    let sq = libm::sqrt(disc);
                    ts.push((-qb - sq) / (2.0 * qa));
                    ts.push((-qb + sq) / (2.0 * qa));
                }
            }
        }
        let mut x = 0;
        for t in ts {
            if near_end(t) {
                return None;
            }
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            let p = a + d * t;
            if self.inside(p)? {
                x -= sign(d.dot(self.normal(p)))?;
            }
        }
        Some(x)
    }
}

/// One trial: a face with its trims stored either way round, a segment.
struct Trial {
    face: Face,
    stored: Vec<(Curve, Coedge)>,
    a: DVec3,
    b: DVec3,
}

impl Trial {
    fn new(rng: &mut Rng) -> Trial {
        let face = Face::random(rng);
        let stored = face
            .trims()
            .into_iter()
            .zip(0u32..)
            .map(|(c, i)| {
                let forward = rng.next_u64().is_multiple_of(2);
                let c = if forward { c } else { c.reversed() };
                (
                    c,
                    Coedge {
                        edge: EdgeId(i),
                        forward,
                    },
                )
            })
            .collect();
        Trial {
            face,
            stored,
            a: rng.point(1.8),
            b: rng.point(1.8),
        }
    }

    fn boundary(&self) -> Vec<Coedge> {
        self.stored.iter().map(|&(_, c)| c).collect()
    }

    /// `x12(e, f)` with `e` the segment of `A`, `f` the face of `B`; the
    /// ends from the ray `down` and `s11` scaled by `s11_sign`.
    fn x12(&self, down: bool, s11_sign: i64) -> Option<i64> {
        let ends = [self.face.ray(self.a, down)?, self.face.ray(self.b, down)?];
        let e = Curve::Line(self.a, self.b);
        let read = |g: EdgeId| {
            s11(&e, &self.stored[g.index()].0)
                .ok_or(())
                .map(|s| s * s11_sign)
        };
        x12(ends, &self.boundary(), read).ok()
    }

    /// `x21(g, f)` with `g` the segment of `B`, `f` the face of `A`.
    fn x21(&self, down: bool, s11_sign: i64) -> Option<i64> {
        let ends = [self.face.ray(self.a, down)?, self.face.ray(self.b, down)?];
        let g = Curve::Line(self.a, self.b);
        let read = |e: EdgeId| {
            s11(&self.stored[e.index()].0, &g)
                .ok_or(())
                .map(|s| s * s11_sign)
        };
        x21(ends, &self.boundary(), read).ok()
    }
}

/// Per convention `(down, s11_sign)`, the trials run, those with a
/// crossing, and the mismatches.
fn run(seeds: std::ops::Range<u64>, x: impl Fn(&Trial, bool, i64) -> Option<i64>) -> [[u64; 3]; 4] {
    let conventions = [(false, 1), (false, -1), (true, 1), (true, -1)];
    let mut out = [[0u64; 3]; 4];
    for seed in seeds {
        let t = Trial::new(&mut Rng::new(seed));
        let Some(truth) = t.face.crossings(t.a, t.b) else {
            continue;
        };
        let got: Option<Vec<i64>> = conventions.iter().map(|&(d, s)| x(&t, d, s)).collect();
        let Some(got) = got else { continue };
        for (o, g) in out.iter_mut().zip(got) {
            o[0] += 1;
            o[1] += u64::from(truth != 0);
            o[2] += u64::from(g != truth);
        }
    }
    out
}

/// `x12`: the upward ray and `−Σ s11` match the face's own crossings on
/// every trial; the other three combinations don't.
#[test]
fn x12_matches_the_crossings_only_in_its_convention() {
    let r = run(varde_testing::seeds(1500, 30000), Trial::x12);
    let [[n, hit, bad], ..] = r;
    assert_eq!(bad, 0, "{r:?}");
    assert!(n > 1000 && hit > 200, "{r:?}");
    for w in &r[1..] {
        assert!(w[2] > 10, "{r:?}");
    }
}

/// `x21`: the downward ray and `−Σ s11`, the same `s11` (`g` above `e`),
/// match on every trial; the three other sign combinations don't.
#[test]
fn x21_matches_the_crossings_only_in_its_convention() {
    let r = run(varde_testing::seeds(1500, 30000), Trial::x21);
    let [_, _, [n, hit, bad], _] = r;
    assert_eq!(bad, 0, "{r:?}");
    assert!(n > 1000 && hit > 200, "{r:?}");
    for (i, w) in r.iter().enumerate() {
        if i != 2 {
            assert!(w[2] > 10, "{r:?}");
        }
    }
}

/// The trials hit both kinds of face, the band's fold and its arcs, and
/// down-facing layers, so the counts above saw them.
#[test]
fn trials_reach_the_fold() {
    let (mut folded, mut arcs) = (0, 0);
    for seed in 0..1500 {
        let t = Trial::new(&mut Rng::new(seed));
        if let Face::Band { .. } = t.face {
            let Some(l) = t.face.layers(t.a) else {
                continue;
            };
            folded += usize::from(l.len() == 2 && l[0].1 != l[1].1);
            let e = Curve::Line(t.a, t.b);
            arcs += usize::from(
                [1, 3]
                    .iter()
                    .any(|&i| s11(&e, &t.stored[i].0).is_some_and(|s| s != 0)),
            );
        }
    }
    assert!(folded > 5 && arcs > 20, "{folded} {arcs}");
}
