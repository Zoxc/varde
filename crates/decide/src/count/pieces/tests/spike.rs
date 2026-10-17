//! A whole face counted from its boxes and pieces against the exact
//! answer: a line tangent to an arc of radius 1 (a cusp, its spike tens
//! of thousands of bands long at `β = 10⁻⁸`) closing a face on the plane
//! `z = x/4 + y/2`. Per query point, boxes refine from the tile: dropped
//! where their shadow misses the query's, split while thin or not
//! certified above or below it; every leaf above the query adds its
//! degree (a core box's, or a strip or corner piece's from its ribbons and
//! the edges' ray records). The sum is the number of times the face lies
//! above the query along `UP`, exactly; the boxes visited stay few
//! whether the query is far from the spike, over it, or beside its tip,
//! and nothing runs out.

use super::*;
use crate::count::boxes::{Corners, Role, box_hull, role, split};
use crate::count::degree::{SidedError, box_degree_sided};
use crate::num::bounded::{down, up};
use crate::perturb::UP;
use crate::records::side::Sides;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode;
use smallvec::SmallVec;

/// The surface: `(u, v) ↦ (u, v, u/4 + v/2)`, one tile `[1, 2] × [0, 1]`.
const TILE: [std::ops::Range<i64>; 2] = [1..2, 0..1];

fn map(u: f64, v: f64) -> [f64; 3] {
    [u, v, u * 0.25 + v * 0.5]
}

fn tile_patch() -> Vec<Patch> {
    let mut net = Vec::new();
    for a in 0..2 {
        for b in 0..2 {
            let [x, y, z] = map((1 + a) as f64, b as f64);
            net.push([x, y, z, 1.0]);
        }
    }
    vec![Patch::from_affine(1, 1, net, &first()).unwrap()]
}

/// The cusp's vertex, off the dyadic lines of every box above the finest
/// (on them: [`spikes_with_their_vertex_on_box_corners_settle_at_once`]).
const W: [f64; 2] = [
    1.0625 + 5.0 / 4_503_599_627_370_496.0,
    0.25 + 3.0 / 18_014_398_509_481_984.0,
];
const SIN60: f64 = 0.8660254037844386;

/// A face of three edges along its loop (counter-clockwise), each one
/// span monotone in `u`: a curve out of `W` (the spike's lower side), a
/// line, a curve back into `W` leaving it the same way.
struct Shape([UvSpan; 3]);

fn mid(x: [f64; 2], y: [f64; 2]) -> [f64; 3] {
    [(x[0] + y[0]) * 0.5, (x[1] + y[1]) * 0.5, 1.0]
}

fn pt(x: [f64; 2]) -> [f64; 3] {
    [x[0], x[1], 1.0]
}

impl Shape {
    /// The line out of `W` along `+u`, up the side, an arc of radius 1
    /// back into `W` tangent to the line (contact order 2).
    fn cusp() -> Shape {
        Shape::cusp_at(W)
    }

    /// The same with its vertex at `w`.
    fn cusp_at(w: [f64; 2]) -> Shape {
        let (a, p) = ([w[0] + SIN60, w[1]], [w[0] + SIN60, w[1] + 0.5]);
        Shape([
            [pt(w), mid(w, a), pt(a)],
            [pt(a), mid(a, p), pt(p)],
            [pt(p), [w[0] + 0.5773502691896257, w[1], SIN60], pt(w)],
        ])
    }

    /// Two lines `2⁻⁶` rad apart (order 1).
    fn acute() -> Shape {
        Shape::acute_at(W)
    }

    /// The same with its vertex at `w`.
    fn acute_at(w: [f64; 2]) -> Shape {
        let slope = crate::num::trig::tan(1.0 / 64.0);
        let (a, b) = ([w[0] + 0.75, w[1]], [w[0] + 0.75, w[1] + 0.75 * slope]);
        Shape([
            [pt(w), mid(w, a), pt(a)],
            [pt(a), mid(a, b), pt(b)],
            [pt(b), mid(b, w), pt(w)],
        ])
    }

    /// Two parabolas of one curvature at `W` (order 3): `(0.7t +
    /// 0.175t², 0.7t²)` out of `W`, `(0.7t, 0.7t²)` back into it.
    fn equal_curvature() -> Shape {
        Shape::equal_curvature_at(W)
    }

    /// The same with its vertex at `w`.
    fn equal_curvature_at(w: [f64; 2]) -> Shape {
        let at = |du: f64, dv: f64| [w[0] + du, w[1] + dv, 1.0];
        let (b, a) = ([w[0] + 0.875, w[1] + 0.7], [w[0] + 0.7, w[1] + 0.7]);
        Shape([
            [at(0.0, 0.0), at(0.35, 0.0), pt(b)],
            [pt(b), mid(b, a), pt(a)],
            [pt(a), at(0.35, 0.0), at(0.0, 0.0)],
        ])
    }

    /// The edges' curves: the p-curves carried onto the plane, the same
    /// weights.
    fn curves(&self) -> Vec<Vec<Span>> {
        self.0
            .iter()
            .map(|s| {
                vec![s.map(|p| {
                    let [x, y, z] = map(p[0], p[1]);
                    [x, y, z, p[2]]
                })]
            })
            .collect()
    }

    fn trims(&self, beta: f64) -> Vec<Trim<'_>> {
        (0..3)
            .map(|k| Trim {
                coedge: CoedgeId(k as u32),
                reversed: false,
                spans: std::slice::from_ref(&self.0[k]),
                ends: [VertId(k as u32), VertId(((k + 1) % 3) as u32)],
                zero_length: false,
                band: beta,
            })
            .collect()
    }

    /// The vertices in the geometric order: by `x` (`u`), then `y`.
    fn order(&self) -> Vec<VertId> {
        let mut v: Vec<(VertId, [f64; 2])> = (0..3)
            .map(|k| (VertId(k as u32), [self.0[k][0][0], self.0[k][0][1]]))
            .collect();
        v.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]).then(a.1[1].total_cmp(&b.1[1])));
        v.into_iter().map(|x| x.0).collect()
    }

    /// The shape mirrored in `u` (`u ↦ 3 − u`) and in `v` (`v ↦ 1 − v`),
    /// each keeping the tile, and its loop reversed (`rev`): the cusp's
    /// vertex stays the first.
    fn mirrored(&self, mu: bool, mv: bool, rev: bool) -> Shape {
        let p = |x: [f64; 3]| {
            [
                if mu { 3.0 - x[0] } else { x[0] },
                if mv { 1.0 - x[1] } else { x[1] },
                x[2],
            ]
        };
        let s = self.0.map(|sp| sp.map(p));
        let back = |sp: &UvSpan| [sp[2], sp[1], sp[0]];
        Shape(if rev {
            [back(&s[2]), back(&s[1]), back(&s[0])]
        } else {
            s
        })
    }

    /// The same curves with span `k`'s weights scaled by `scale[k]` and
    /// reparametrised by `r` (weights times `1, r, r²`).
    fn weighted(&self, scale: [f64; 3], r: f64) -> Shape {
        let mut s = Shape(self.0);
        for (sp, c) in s.0.iter_mut().zip(scale) {
            for (i, p) in sp.iter_mut().enumerate() {
                p[2] *= c * [1.0, r, r * r][i];
            }
        }
        s
    }

    /// The cusp's vertex.
    fn w(&self) -> [f64; 2] {
        [self.0[0][0][0], self.0[0][0][1]]
    }

    /// The `u` at `s` along the spike from the vertex.
    fn along(&self, s: f64) -> f64 {
        let w = self.w()[0];
        if self.0[0][2][0] > w { w + s } else { w - s }
    }

    /// The spike's lower and upper sides at `u = W_u + s`.
    fn sides_at(&self, s: f64) -> [f64; 2] {
        let back = |sp: &UvSpan| [sp[2], sp[1], sp[0]];
        [
            graph(&self.0[0], self.along(s)).expect("on the lower side"),
            graph(&back(&self.0[2]), self.along(s)).expect("on the upper side"),
        ]
    }

    /// The winding of the face's loop about the UV point `x` (`None`
    /// within `m` of a curve along `v`), from the crossings of the ray
    /// from `x` along `+v`: each edge, monotone in `u`, crosses the line
    /// `u = x_u` at most once.
    fn winding(&self, x: [f64; 2], m: f64) -> Option<i64> {
        let mut w = 0i64;
        for sp in &self.0 {
            let (u0, u2) = (sp[0][0], sp[2][0]);
            if u0 == u2 {
                if (x[0] - u0).abs() < m {
                    return None;
                }
                continue;
            }
            let (lo, hi) = if u0 < u2 { (u0, u2) } else { (u2, u0) };
            if x[0] <= lo || x[0] >= hi {
                if (x[0] - lo).abs() < m || (x[0] - hi).abs() < m {
                    return None;
                }
                continue;
            }
            let v = graph(sp, x[0]).expect("monotone");
            if (v - x[1]).abs() < m {
                return None;
            }
            if v > x[1] {
                // Above `x` moving towards `−u`: counter-clockwise.
                w += if u2 < u0 { 1 } else { -1 };
            }
        }
        Some(w)
    }
}

/// The point of a span at `t`, in floats.
fn at_t(s: &UvSpan, t: f64) -> [f64; 2] {
    let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
    let w: f64 = (0..3).map(|i| b[i] * s[i][2]).sum();
    let c = |k: usize| (0..3).map(|i| b[i] * s[i][2] * s[i][k]).sum::<f64>() / w;
    [c(0), c(1)]
}

/// The `v` of a span monotone in `u` where its `u` is `u`, by bisection.
fn graph(sp: &UvSpan, u: f64) -> Option<f64> {
    let inc = sp[2][0] > sp[0][0];
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if (at_t(sp, mid)[0] < u) == inc {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let p = at_t(sp, 0.5 * (lo + hi));
    ((p[0] - u).abs() < 1e-12).then_some(p[1])
}

/// The ray record of the query `q` (of `B`, its ray `+r̂`) against a
/// rational quadratic span chain: the roots of `ν·P − (ν·q)·W` in
/// closed form, each bracketed where the exact sign changes.
fn rays_q(c: &[Span], q: [f64; 3]) -> ChainRec {
    use malachite::Rational as Q;
    let qq = |x: f64| Q::try_from(x).unwrap();
    let dot = |a: DVec3, p: [f64; 4]| -> Q {
        (qq(a.x) * qq(p[0]) + qq(a.y) * qq(p[1]) + qq(a.z) * qq(p[2])) * qq(p[3])
    };
    let dq = |a: DVec3| qq(a.x) * qq(q[0]) + qq(a.y) * qq(q[1]) + qq(a.z) * qq(q[2]);
    let mut out = ChainRec::default();
    for (k, sp) in c.iter().enumerate() {
        // `f(a)(t) = Σ (a·Pᵢ − a·q)·wᵢ·Bᵢ(t)`, exact in Bernstein form.
        let bern = |a: DVec3| -> [Q; 3] {
            let aq = dq(a);
            std::array::from_fn(|i| dot(a, sp[i]) - &aq * qq(sp[i][3]))
        };
        let eval = |b: &[Q; 3], t: f64| -> Q {
            let t = qq(t);
            let s = Q::from(1) - &t;
            &b[0] * &s * &s + Q::from(2) * &b[1] * &s * &t + &b[2] * &t * &t
        };
        let g = bern(NU);
        let h = bern(R_HAT);
        let f = |x: &Q| f64::rounding_from(x, RoundingMode::Nearest).0;
        let (b0, b1, b2) = (f(&g[0]), f(&g[1]), f(&g[2]));
        // Power form: `c + b t + a t²`.
        let (cc, bb, aa) = (b0, 2.0 * (b1 - b0), b0 - 2.0 * b1 + b2);
        let mut ts: Vec<f64> = Vec::new();
        if aa == 0.0 {
            if bb != 0.0 {
                ts.push(-cc / bb);
            }
        } else {
            let disc = bb * bb - 4.0 * aa * cc;
            if disc > 0.0 {
                let r = -(bb + bb.signum() * disc.sqrt()) / 2.0;
                ts.extend([r / aa, cc / r]);
            }
        }
        ts.sort_by(f64::total_cmp);
        for t in ts {
            if !(-1e-9..=1.0 + 1e-9).contains(&t) {
                continue;
            }
            // Bracket the root where `g` changes sign exactly.
            let mut e = 1e-15;
            let bracket = loop {
                let (lo, hi) = (
                    if t - e > 0.0 { t - e } else { 0.0 },
                    if t + e < 1.0 { t + e } else { 1.0 },
                );
                let (gl, gh) = (eval(&g, lo), eval(&g, hi));
                if (gl < 0) != (gh < 0) && gl != 0 && gh != 0 {
                    break Some([lo, hi]);
                }
                e *= 4.0;
                if e > 1e-6 {
                    break None;
                }
            };
            let Some([lo, hi]) = bracket else {
                continue;
            };
            if eval(&h, lo) <= 0 || eval(&h, hi) <= 0 {
                assert!(
                    eval(&h, lo) < 0 && eval(&h, hi) < 0,
                    "a crossing next to the query"
                );
                continue;
            }
            let sign = if eval(&g, hi) > 0 {
                Sign::Pos
            } else {
                Sign::Neg
            };
            out.crossings.push(ChainCrossing {
                span: k as u32,
                crossing: Crossing { t: [lo, hi], sign },
            });
        }
    }
    out
}

/// The tilted plane's tile edges and corners as the side records read
/// them, and the query point.
struct Tilted {
    q: DVec3,
}

impl ShadowGeometry for Tilted {
    fn point(&self, p: Point) -> Moving {
        assert_eq!(p, point());
        Moving::fixed(self.q)
    }

    fn near_radius(&self, _: Point) -> f64 {
        0.0
    }

    fn ends(&self, _: Operand, _: EdgeId) -> [VertId; 2] {
        unreachable!("no edges here")
    }

    fn line(&self, _: Point, _: EdgeId) -> Line {
        unreachable!("no edges here")
    }
}

impl SideGeometry for Tilted {
    fn tile_edge(&self, e: &TileEdge) -> Option<Span> {
        let at = |c: f64| {
            let [x, y, z] = match e.fixed {
                Dir::U => map(e.line as f64, c),
                Dir::V => map(c, e.line as f64),
            };
            [x, y, z, 1.0]
        };
        let c = e.cell as f64;
        Some([at(c), at(c + 0.5), at(c + 1.0)])
    }

    fn corner(&self, _: Point, c: &TileCorner) -> Option<Moving> {
        let [x, y, z] = map(c.at[0] as f64, c.at[1] as f64);
        Some(Moving::of_a(DVec3::new(x, y, z), Sign::Pos, D))
    }

    fn edge_rates<S: Curved>(&self, _: Point, _: &TileEdge, t: &S) -> [[S; 3]; ORDERS] {
        let lit = |v: DVec3| [v.x, v.y, v.z].map(|x| S::from_f64(x, t));
        [lit(D), lit(T[0]), lit(T[1]), lit(T[2])]
    }
}

/// `f` with the side records it asks for, decided as it asks.
fn settled<R, E>(
    q: [f64; 3],
    need: impl Fn(&E) -> Option<SideKey>,
    f: impl Fn(&Sides) -> Result<R, E>,
) -> Result<R, E> {
    let geom = Tilted {
        q: DVec3::from_array(q),
    };
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).unwrap();
    let mut sides = Sides::new();
    for _ in 0..8 {
        match f(&sides) {
            Err(e) if need(&e).is_some() => {
                sides
                    .fill([need(&e).unwrap()], &geom, band, &mut Seq)
                    .unwrap();
            }
            r => return r,
        }
    }
    panic!("records asked again after filling")
}

/// The interval of `a·x` over the hull, outward.
fn lin(h: &HullBox, a: DVec3) -> [f64; 2] {
    let c = [a.x, a.y, a.z];
    let (mut lo, mut hi) = (Bounded::exact(0.0), Bounded::exact(0.0));
    for k in 0..3 {
        let (l, r) = if c[k] >= 0.0 {
            (h.lo[k], h.hi[k])
        } else {
            (h.hi[k], h.lo[k])
        };
        lo = lo + Bounded::exact(c[k]) * Bounded::exact(l);
        hi = hi + Bounded::exact(c[k]) * Bounded::exact(r);
    }
    [lo.enclose()[0], hi.enclose()[1]]
}

/// `a·q`, outward.
fn lin_q(q: [f64; 3], a: DVec3) -> [f64; 2] {
    let s = Bounded::exact(a.x) * Bounded::exact(q[0])
        + Bounded::exact(a.y) * Bounded::exact(q[1])
        + Bounded::exact(a.z) * Bounded::exact(q[2]);
    s.enclose()
}

/// The count of `shape` above `q` along `UP`, the boxes visited, and the
/// corner boxes holding the vertex (in their closed UV box) whose piece
/// didn't settle.
fn count(shape: &Shape, q: [f64; 3], beta: f64) -> (i64, usize, usize) {
    let cs = shape.curves();
    let trims = shape.trims(beta);
    let face = FaceUv::new(Operand::A, FaceId(3), TILE, [1.0, 1.0], &trims, false).unwrap();
    let patches = tile_patch();
    let corners = Corners::new(&face, &patches, &shape.order()).unwrap();
    assert!(
        corners.cusp(VertId(0)).is_some(),
        "the sharp corner's record"
    );
    let recs: Vec<ChainRec> = cs.iter().map(|c| rays_q(c, q)).collect();
    let map1 = [SpanMap {
        span: 0,
        t: [0.0, 1.0],
    }];
    let qs = QShadow::of(q).unwrap();
    let (up_q, r_q, n_q) = (lin_q(q, UP), lin_q(q, R_HAT), lin_q(q, NU));
    let mut queue: Vec<(BoxKey, Option<SmallVec<[crate::count::boxes::SpanRef; 8]>>)> =
        face.tile_boxes().map(|b| (b, None)).collect();
    let (mut sum, mut visited, mut stuck) = (0i64, 0usize, 0usize);
    let w = shape.w();
    let holds_w = |b: &BoxKey| {
        [Dir::U, Dir::V].into_iter().all(|d| {
            let k = usize::from(d == Dir::V);
            let (i, e) = (b.tile()[k] as f64, b.extent(d));
            i + dy(e.lo()) <= w[k] && w[k] <= i + dy(e.hi())
        })
    };
    while let Some((b, among)) = queue.pop() {
        visited += 1;
        assert!(visited < 20_000, "refinement runs away at q = {q:?}");
        let patch = &patches[0];
        let mut h = box_hull(patch, &b).expect("positive weights");
        // The ribbons lie within a band of the box's hull.
        for k in 0..3 {
            h.lo[k] = down(h.lo[k] - beta);
            h.hi[k] = up(h.hi[k] + beta);
        }
        let (r, n) = (lin(&h, R_HAT), lin(&h, NU));
        if r[1] < r_q[0] || r[0] > r_q[1] || n[1] < n_q[0] || n[0] > n_q[1] {
            continue;
        }
        let rl = role(&face, &corners, &b, &first(), among.as_deref()).expect("a role");
        let z = lin(&h, UP);
        let above = z[0] > up_q[1];
        let clear = above || z[1] < up_q[0];
        let degree = match (&rl.role, clear) {
            (Role::Outside, _) => continue,
            (_, true) if !above => continue,
            (Role::Core, true) => settled(
                q,
                |e: &SidedError| match e {
                    SidedError::Need(k) => Some(*k),
                    _ => None,
                },
                |sides| box_degree_sided(&face, &b, patch, &qs, point(), sides),
            )
            .map(|d| d.degree)
            .ok(),
            (Role::Strip(c) | Role::Corner { met: c, .. }, true) => {
                let ribbons: Vec<Ribbon<'_>> = c
                    .iter()
                    .map(|c| Ribbon {
                        coedge: *c,
                        curve: &cs[c.index()],
                        map: &map1,
                        rays: &recs[c.index()],
                    })
                    .collect();
                match settled(
                    q,
                    |e: &PieceError| match e {
                        PieceError::Need(k) => Some(*k),
                        _ => None,
                    },
                    |sides| strip_degree(&face, &patches, &[b], &ribbons, &qs, point(), sides),
                ) {
                    Ok(d) => Some(d.degree),
                    Err(PieceError::Unsettled | PieceError::Through) => {
                        if matches!(rl.role, Role::Corner { .. }) && holds_w(&b) {
                            stuck += 1;
                        }
                        None
                    }
                    Err(e) => panic!("{e:?} at box {b:?}, q = {q:?}"),
                }
            }
            _ => None,
        };
        match degree {
            Some(d) => sum += d,
            None => {
                assert!(
                    b.depth() < 52,
                    "box {b:?} too deep at q = {q:?}: {:?}",
                    rl.role
                );
                let s = split(&face, &b, patch, Some(&qs)).expect("a split");
                for half in s.halves {
                    queue.push((half, Some(rl.met.clone())));
                }
            }
        }
    }
    (sum, visited, stuck)
}

/// The query `δ` along `UP` below (`δ > 0`) or above the face's plane at
/// the UV point `x`.
fn query_at(x: [f64; 2], delta: f64) -> [f64; 3] {
    let p = map(x[0], x[1]);
    let n = UP.length();
    [
        p[0] - delta * UP.x / n,
        p[1] - delta * UP.y / n,
        p[2] - delta * UP.z / n,
    ]
}

/// The truth at `q`: whether the line along `UP` through it meets the
/// face above it (`None` too near the boundary or the plane).
fn truth(shape: &Shape, q: [f64; 3]) -> Option<i64> {
    // `q + λ·UP` on the plane: `λ = (x/4 + y/2 − z)/30`.
    let lambda = (q[0] * 0.25 + q[1] * 0.5 - q[2]) / (UP.z - 0.25 * UP.x - 0.5 * UP.y);
    let x = [q[0] + lambda * UP.x, q[1] + lambda * UP.y];
    if lambda.abs() < 1e-12 {
        return None;
    }
    let w = shape.winding(x, 1e-13)?;
    Some(if lambda > 0.0 { w } else { 0 })
}

/// Counts random queries over `shape` (away from the spike, over it from
/// near its tip to past its cut, beside it) against the truth, each in
/// at most `most` boxes.
fn check_shape(shape: &Shape, seed: u64, most: usize) {
    let beta = 1e-8;
    let mut rng = Rng::new(seed);
    let s_beta = {
        let trims = shape.trims(beta);
        let face = FaceUv::new(Operand::A, FaceId(3), TILE, [1.0, 1.0], &trims, false).unwrap();
        let c = Corners::new(&face, &tile_patch(), &shape.order()).unwrap();
        c.cusp(VertId(0)).expect("a sharp corner").s_beta_f64()
    };
    // The shorter side's extent along `u`.
    let w = shape.w()[0];
    let (f0, f2) = ((shape.0[0][2][0] - w).abs(), (shape.0[2][0][0] - w).abs());
    let far = if f0 < f2 { f0 } else { f2 };
    let mut cases: Vec<([f64; 3], &str)> = Vec::new();
    let sign = |rng: &mut Rng| if rng.unit() < 0.5 { 1.0 } else { -1.0 };
    for _ in 0..8 {
        let s = rng.range(0.1, 0.95) * far;
        let [lo, hi] = shape.sides_at(s);
        let x = [shape.along(s), lo + (hi - lo) * rng.range(0.05, 0.95)];
        let d = rng.range(0.01, 0.3) * sign(&mut rng);
        cases.push((query_at(x, d), "face"));
    }
    for _ in 0..8 {
        let x = [rng.range(1.05, 1.95), rng.range(0.05, 0.95)];
        let d = rng.range(0.01, 0.3) * sign(&mut rng);
        cases.push((query_at(x, d), "away"));
    }
    for _ in 0..12 {
        let s = s_beta * rng.log_range(0.01, 3.0);
        let s = if s < 0.9 * far { s } else { 0.9 * far };
        let [lo, hi] = shape.sides_at(s);
        let x = [shape.along(s), lo + (hi - lo) * rng.range(0.2, 0.8)];
        let d = rng.range(1e-4, 1e-2) * sign(&mut rng);
        cases.push((query_at(x, d), "spike"));
    }
    for _ in 0..6 {
        let s = s_beta * rng.range(0.1, 2.0);
        let [lo, hi] = shape.sides_at(s);
        let x = [shape.along(s), lo - (hi - lo) * rng.range(0.2, 2.0)];
        cases.push((query_at(x, 1e-3), "beside"));
    }
    let mut checked = 0;
    for (q, kind) in cases {
        let Some(want) = truth(shape, q) else {
            continue;
        };
        let (got, visited, stuck) = count(shape, q, beta);
        assert_eq!(got, want, "{kind} q = {q:?}");
        assert!(visited < most, "{kind} q = {q:?}: {visited} boxes");
        assert_eq!(stuck, 0, "{kind} q = {q:?}: corner boxes at the vertex");
        checked += 1;
    }
    assert!(checked >= 30, "{checked}");
}

#[test]
fn a_cusp_counts_in_few_pieces_at_a_tight_band() {
    check_shape(&Shape::cusp(), 17, 1000);
}

#[test]
fn an_acute_corner_counts_in_few_pieces() {
    check_shape(&Shape::acute(), 18, 1000);
}

#[test]
fn equal_curvatures_count_in_few_pieces() {
    check_shape(&Shape::equal_curvature(), 19, 1000);
}

/// The spikes mirrored (towards `−u`, below their vertex) and their loops
/// reversed, so the curve into the vertex and the one out of it swap.
#[test]
fn mirrored_spikes_count_alike() {
    let shapes = [Shape::cusp(), Shape::acute(), Shape::equal_curvature()];
    // The counter-clockwise ones: an even number of reflections.
    for m in [3u64, 5, 6] {
        let s = shapes[0].mirrored(m & 1 != 0, m & 2 != 0, m & 4 != 0);
        check_shape(&s, 40 + m, 1000);
    }
    check_shape(&shapes[1].mirrored(true, false, true), 50, 1000);
    check_shape(&shapes[2].mirrored(false, true, true), 51, 1000);
}

/// The spikes' curves with weights scaled by `1/64` to `64` and
/// reparametrised (`r = 8`, `⅛`): a cut where a side meets a curve almost
/// along it sits in a root interval the isolation narrows little, which
/// the cut halves until it places the crossing along the side.
#[test]
fn weighted_spikes_count_alike() {
    let shapes = [Shape::cusp(), Shape::acute(), Shape::equal_curvature()];
    for (k, (scale, r)) in [
        ([1.0 / 64.0, 64.0, 1.0], 1.0),
        ([64.0, 1.0, 1.0 / 64.0], 0.125),
        ([1.0, 1.0 / 64.0, 64.0], 8.0),
    ]
    .into_iter()
    .enumerate()
    {
        check_shape(&shapes[k].weighted(scale, r), 60 + k as u64, 1000);
    }
}

/// The spikes with their vertex on the dyadic grid, `(1 1/16, 1/4)`, a
/// corner of every box from depth 4 in `u` and 2 in `v` on (box lines read
/// as moved to their high side: the vertex lies in the box below and left
/// of it, the curves leave it through that box's sides at one point,
/// ordered by their shapes, and a box only touching the vertex is inside
/// or outside by the face's sector there): every corner box holding the
/// vertex settles where it is first reached, none refines for it, and the
/// counts are exact. The cusp also mirrored, its spike towards `−u`.
#[test]
fn spikes_with_their_vertex_on_box_corners_settle_at_once() {
    let w = [1.0625, 0.25];
    let shapes = [
        Shape::cusp_at(w),
        Shape::acute_at(w),
        Shape::equal_curvature_at(w),
    ];
    for (k, s) in shapes.iter().enumerate() {
        check_shape(s, 70 + k as u64, 1000);
    }
    for m in [3u64, 5, 6] {
        let s = shapes[0].mirrored(m & 1 != 0, m & 2 != 0, m & 4 != 0);
        check_shape(&s, 80 + m, 1000);
    }
}
