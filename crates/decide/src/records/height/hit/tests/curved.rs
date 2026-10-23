//! Column hits on curved tiles: a cylinder's and a sphere's tile trimmed
//! by rational quadratic p-curves off their chords, the edges' curves off
//! the surface (the ribbons have width), against the exact ray oracle;
//! and the fold rule where the column touches a cylinder at `q`.

use super::*;
use crate::num::decide::Sign;
use crate::perturb::{rate_rational, rates_a};

/// A circle's arc of 90° from angle `a0` (radians) about `c` in the plane
/// of `e1`, `e2` (orthonormal), radius `r`, as a rational quadratic.
fn arc(c: DVec3, e1: DVec3, e2: DVec3, r: f64, a0: f64) -> [[f64; 4]; 3] {
    let at = |a: f64| {
        let (s, co) = crate::num::trig::sin_cos(a);
        c + (e1 * co + e2 * s) * r
    };
    let h = std::f64::consts::FRAC_PI_4;
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let (p0, p2) = (at(a0), at(a0 + 2.0 * h));
    // The tangents' meet: the middle direction at `r / cos 45°`.
    let p1 = c + (at(a0 + h) - c) / w;
    let pt = |p: DVec3, wt: f64| [p.x, p.y, p.z, wt];
    [pt(p0, 1.0), pt(p1, w), pt(p2, 1.0)]
}

/// A random orthonormal frame.
fn frame(rng: &mut Rng) -> [DVec3; 3] {
    let a = rng.direction();
    let b = a.any_orthonormal_vector();
    [b, a.cross(b), a]
}

/// A cylinder's tile: `u` along a 90° arc, `v` along the axis (`e3`).
fn cylinder(rng: &mut Rng) -> TileNet {
    let [e1, e2, e3] = frame(rng);
    let c = rng.direction() * 0.1;
    let r = rng.range(0.5, 1.5);
    let len = rng.range(1.0, 2.0);
    let a = arc(c, e1, e2, r, rng.range(-3.0, 3.0));
    std::array::from_fn(|n| {
        let (i, j) = (n / 3, n % 3);
        let p = DVec3::new(a[i][0], a[i][1], a[i][2]) + e3 * (len * j as f64 / 2.0 - len / 2.0);
        [p.x, p.y, p.z, a[i][3]]
    })
}

/// A sphere's tile: `u` along a meridian's 90° arc (latitudes −45° to
/// 45°), `v` along a parallel's 90°.
fn sphere(rng: &mut Rng) -> TileNet {
    let [e1, e2, e3] = frame(rng);
    let c = rng.direction() * 0.1;
    let r = rng.range(0.5, 1.5);
    let a0 = rng.range(-3.0, 3.0);
    // The meridian in the `(ρ, z)` plane and the parallel in the unit
    // circle's: the net is their product.
    let m = arc(
        DVec3::ZERO,
        DVec3::X,
        DVec3::Z,
        r,
        -std::f64::consts::FRAC_PI_4,
    );
    let p = arc(DVec3::ZERO, e1, e2, 1.0, a0);
    std::array::from_fn(|n| {
        let (i, j) = (n / 3, n % 3);
        let (rho, z) = (m[i][0], m[i][2]);
        let x = c + DVec3::new(p[j][0], p[j][1], p[j][2]) * rho + e3 * z;
        let wt = m[i][3] * p[j][3];
        [x.x, x.y, x.z, wt]
    })
}

/// The net's point at `(u, v)`, in `f64`.
fn net_at(net: &TileNet, u: f64, v: f64) -> DVec3 {
    let b = |t: f64| [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
    let (bu, bv) = (b(u), b(v));
    let (mut p, mut w) = (DVec3::ZERO, 0.0);
    for i in 0..3 {
        for j in 0..3 {
            let c = net[3 * i + j];
            let k = bu[i] * bv[j] * c[3];
            p += DVec3::new(c[0], c[1], c[2]) * k;
            w += k;
        }
    }
    p / w
}

/// A curved test face: one tile, a convex quad of rational quadratic
/// p-curves pushed off their chords (middle weights `1/2` to `2`), the
/// edges' curves through the surface's points at the corners and near
/// its point at each p-curve's middle, lifted off it by up to `lift`.
struct CurvedFace {
    nets: Vec<TileNet>,
    pspans: Vec<UvSpan>,
    curves: Vec<Span>,
    reversed: Vec<bool>,
    maps: Vec<SpanMap>,
    inside: i64,
}

impl CurvedFace {
    fn random(rng: &mut Rng, net: TileNet, push: f64, lift: f64) -> CurvedFace {
        let corners: Vec<[f64; 2]> = (0..4)
            .map(|k| {
                let a = std::f64::consts::FRAC_PI_2 * k as f64 + rng.range(-0.4, 0.4);
                let r = rng.range(0.25, 0.4);
                let (s, c) = crate::num::trig::sin_cos(a);
                [0.5 + r * c, 0.5 + r * s].map(|x| (x * 256.0).round() / 256.0)
            })
            .collect();
        // Counter-clockwise, or clockwise on a flipped face.
        let flipped = rng.next_u64() % 2 == 1;
        let (mut pspans, mut curves, mut reversed) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..4 {
            let (ka, kb) = (k, (k + 1) % 4);
            let (ka, kb) = if flipped { (kb, ka) } else { (ka, kb) };
            // Stored along the loop or against it.
            let back = rng.next_u64() % 2 == 1;
            let (ka, kb) = if back { (kb, ka) } else { (ka, kb) };
            let (a, b) = (corners[ka], corners[kb]);
            let off = [dyadic(rng, -push, push, 20), dyadic(rng, -push, push, 20)];
            let m = [0.5 * (a[0] + b[0]) + off[0], 0.5 * (a[1] + b[1]) + off[1]];
            let wm = dyadic(rng, 0.5, 2.0, 8);
            let sp: UvSpan = [[a[0], a[1], 1.0], [m[0], m[1], wm], [b[0], b[1], 1.0]];
            // The p-curve's point at `½`.
            let half = [0, 1].map(|i| (a[i] + 2.0 * wm * m[i] + b[i]) / (2.0 + 2.0 * wm));
            let (pa, pb) = (net_at(&net, a[0], a[1]), net_at(&net, b[0], b[1]));
            let pm = net_at(&net, half[0], half[1]) + rng.direction() * rng.range(0.0, lift);
            let c1 = pm * 2.0 - (pa + pb) * 0.5;
            curves.push([
                [pa.x, pa.y, pa.z, 1.0],
                [c1.x, c1.y, c1.z, 1.0],
                [pb.x, pb.y, pb.z, 1.0],
            ]);
            pspans.push(sp);
            reversed.push(back);
        }
        CurvedFace {
            nets: vec![net],
            pspans,
            curves,
            reversed,
            maps: vec![
                SpanMap {
                    span: 0,
                    t: [0.0, 1.0]
                };
                4
            ],
            inside: if flipped { -1 } else { 1 },
        }
    }

    fn hit_face(&self) -> HitFace<'_> {
        HitFace {
            operand: Operand::B,
            face: FaceId(0),
            tiles: [0..1, 0..1],
            nets: &self.nets,
            inside: self.inside,
            trims: (0..4)
                .map(|k| HitTrim {
                    coedge: CoedgeId(k as u32),
                    edge: EdgeId(k as u32),
                    reversed: self.reversed[k],
                    ends: [VertId(0), VertId(1)],
                    along: Along::Stored,
                    band: 1e-3,
                    pspans: std::slice::from_ref(&self.pspans[k]),
                    curve: std::slice::from_ref(&self.curves[k]),
                    map: std::slice::from_ref(&self.maps[k]),
                })
                .collect(),
        }
    }

    fn oracle(&self) -> ExactFace {
        let loops = vec![
            (0..4)
                .map(|k| Coedge {
                    pcurve: UvChain {
                        spans: vec![self.pspans[k]],
                        knots: vec![0.0, 1.0],
                    },
                    curve: Chain {
                        spans: vec![self.curves[k]],
                        knots: vec![0.0, 1.0],
                    },
                    t_range: [0.0, 1.0],
                    reversed: self.reversed[k],
                })
                .collect(),
        ];
        ExactFace::new(&Face {
            tiles: vec![([0, 0], self.nets[0])],
            loops,
        })
    }
}

/// Columns over random curved faces, near their edges and anywhere over
/// them: the hits' count is the oracle's.
fn check_curved(seed: u64, surface: fn(&mut Rng) -> TileNet, faces: usize) {
    let mut rng = Rng::new(seed);
    let up = [UP.x, UP.y, UP.z];
    let (mut checked, mut ribbon, mut tiles) = (0, 0, 0);
    for _ in 0..faces {
        let net = surface(&mut rng);
        let cf = CurvedFace::random(&mut rng, net, 1.0 / 16.0, 1.0 / 32.0);
        let face = cf.hit_face();
        let oracle = cf.oracle();
        for _ in 0..12 {
            let q = if rng.next_u64().is_multiple_of(3) {
                net_at(&net, rng.range(0.0, 1.0), rng.range(0.0, 1.0))
                    + rng.direction() * rng.range(0.0, 0.5)
            } else {
                // Next to an edge's curve.
                let c = cf.curves[(rng.next_u64() % 4) as usize];
                let t = rng.range(0.05, 0.95);
                let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
                let p = (0..3).fold(DVec3::ZERO, |s, i| {
                    s + DVec3::new(c[i][0], c[i][1], c[i][2]) * b[i]
                });
                p + rng.direction() * rng.range(0.0, 0.05)
            };
            let hits = face_hits(&face, q.to_array(), &motion_of(Operand::A))
                .unwrap_or_else(|e| panic!("q {q:?}: {e:?}"));
            let Some(mine) = count_up(&hits) else {
                continue;
            };
            let c = oracle
                .ray(&Shift::NONE, q.to_array(), &Shift::NONE, up)
                .expect("settled");
            if c.tie {
                continue;
            }
            assert_eq!(mine, c.count, "seed {seed}, q {q:?}, hits {hits:#?}");
            checked += 1;
            ribbon += usize::from(hits.iter().any(|h| matches!(h.sheet, Sheet::Ribbon(..))));
            tiles += usize::from(hits.iter().any(|h| matches!(h.sheet, Sheet::Tile(..))));
        }
    }
    assert!(checked > faces * 8, "{checked}");
    assert!(ribbon > faces / 2, "{ribbon}");
    assert!(tiles > faces * 2, "{tiles}");
}

#[test]
fn hits_on_a_cylinder_count_as_the_oracle() {
    check_curved(0xc71, cylinder, varde_testing::pick(12, 40));
}

#[test]
fn hits_on_a_sphere_count_as_the_oracle() {
    check_curved(0x5f3, sphere, varde_testing::pick(8, 30));
}

/// An elliptic cylinder's tile whose middle ruling, `u = ½`, is a
/// silhouette: the arc in the plane of `r̂` and `ê` (`P₂ − P₀ ∥ ê`, weight
/// ½ in the middle, so its point at `u = ½` is `C + 2a·r̂` exactly), the
/// axis `r̂ + ν` across `ê` (oblique to the arc's plane, so neither of
/// the column's equations is of one coordinate alone). `bend` the side
/// the arc bulges to along `r̂`.
fn fold_tile(bend: f64) -> TileNet {
    let (r, n, up) = (R_HAT, NU, UP);
    let c = DVec3::new(0.25, -0.5, 0.125);
    let (a, b) = (bend / 64.0, 1.0 / 128.0);
    let p0 = c + r * a - up * b;
    let p1 = c + r * (4.0 * a);
    let p2 = c + r * a + up * b;
    let arc = [(p0, 1.0), (p1, 0.5), (p2, 1.0)];
    std::array::from_fn(|k| {
        let (i, j) = (k / 3, k % 3);
        let (p, w) = arc[i];
        let x = p + (r + n) * (j as f64 / 1024.0);
        [x.x, x.y, x.z, w]
    })
}

/// A face of one fold tile, trimmed to `[⅛, ⅞]²` by straight trims on
/// the surface's own iso lines (the edges' curves the surface's there).
fn fold_face(net: TileNet, flipped: bool) -> CurvedFace {
    fold_face_over(net, flipped, 0.125)
}

/// [`fold_face`] trimmed to `[u0, ⅞] × [⅛, ⅞]`.
fn fold_face_over(net: TileNet, flipped: bool, u0: f64) -> CurvedFace {
    let lo = 0.125;
    let hi = 0.875;
    let mut corners = [[u0, lo], [hi, lo], [hi, hi], [u0, hi]];
    if flipped {
        corners.reverse();
    }
    let (mut pspans, mut curves) = (Vec::new(), Vec::new());
    for k in 0..4 {
        let (a, b) = (corners[k], corners[(k + 1) % 4]);
        let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
        pspans.push([[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]);
        let (pa, pb) = (net_at(&net, a[0], a[1]), net_at(&net, b[0], b[1]));
        let pm = net_at(&net, m[0], m[1]) * 2.0 - (pa + pb) * 0.5;
        curves.push([
            [pa.x, pa.y, pa.z, 1.0],
            [pm.x, pm.y, pm.z, 1.0],
            [pb.x, pb.y, pb.z, 1.0],
        ]);
    }
    CurvedFace {
        nets: vec![net],
        pspans,
        curves,
        reversed: vec![false; 4],
        maps: vec![
            SpanMap {
                span: 0,
                t: [0.0, 1.0]
            };
            4
        ],
        inside: if flipped { -1 } else { 1 },
    }
}

#[test]
fn a_column_tangent_to_a_cylinder_at_q_takes_the_fold_rule() {
    // `q` on the silhouette ruling: the column touches the tile at `q`.
    // The moved column enters the fold where it moves back from the side
    // the arc bulges to, and there meets two sheets, `q` between them: the
    // upper one's facing counts upward. Else it misses the tile there.
    let up = [UP.x, UP.y, UP.z];
    let c = DVec3::new(0.25, -0.5, 0.125);
    for bend in [1.0, -1.0] {
        let net = fold_tile(bend);
        for flipped in [false, true] {
            let cf = fold_face(net, flipped);
            let face = cf.hit_face();
            let oracle = cf.oracle();
            // The tile's point at `(½, ½)`, exactly.
            let q = c + R_HAT * (2.0 * bend / 64.0) + (R_HAT + NU) * (1.0 / 1024.0);
            for d in [R_HAT, -R_HAT, R_HAT + UP, -R_HAT + UP * 4.0] {
                let rates = rates_a(Sign::Pos, d);
                let motion: Motion = std::array::from_fn(|k| rate_rational(&rates[k]).map(|x| -x));
                let hits = face_hits(&face, q.to_array(), &motion).expect("a fold");
                let enters = d.dot(R_HAT) * bend < 0.0;
                assert_eq!(hits.len(), if enters { 2 } else { 0 }, "{hits:#?}");
                let mine = count_up(&hits).expect("heights");
                let c = oracle
                    .ray(&Shift::NONE, q.to_array(), &Shift::tie(d.to_array(), 1), up)
                    .expect("settled");
                assert_eq!(mine, c.count, "bend {bend}, flipped {flipped}, d {d:?}");
                // Entering, the count is the upper sheet's facing: up where
                // the face bends away from its normal going up.
                if enters {
                    assert_eq!(mine.abs(), 1);
                }
            }
        }
    }
}

#[test]
fn a_fold_moved_along_its_silhouette_is_decided_by_the_later_orders() {
    // `q` on the silhouette ruling, moved along it at first order (the
    // lateral row's order 1 exactly zero): the fixed translations decide
    // whether the column enters, for `±d` alike, as the oracle counts the
    // moved configuration, both in the limit and at the tie scale
    // `2^-28` (the ruling straight, so the motion along it keeps `q` on
    // the silhouette exactly).
    let up = [UP.x, UP.y, UP.z];
    let c = DVec3::new(0.25, -0.5, 0.125);
    let d = R_HAT + NU;
    let t = crate::perturb::T;
    let mut entered = 0;
    for bend in [1.0, -1.0] {
        let net = fold_tile(bend);
        let q = c + R_HAT * (2.0 * bend / 64.0) + (R_HAT + NU) * (1.0 / 1024.0);
        for flipped in [false, true] {
            let cf = fold_face(net, flipped);
            let face = cf.hit_face();
            let oracle = cf.oracle();
            for (sigma, sign) in [(1i8, Sign::Pos), (-1, Sign::Neg)] {
                let rates = rates_a(sign, d);
                let motion: Motion = std::array::from_fn(|k| rate_rational(&rates[k]).map(|x| -x));
                let hits = face_hits(&face, q.to_array(), &motion).expect("a fold");
                assert!(hits.len() == 2 || hits.is_empty(), "{hits:#?}");
                entered += usize::from(!hits.is_empty());
                let mine = count_up(&hits).expect("heights");
                let limit = oracle
                    .ray(
                        &Shift::NONE,
                        q.to_array(),
                        &Shift::tie(d.to_array(), sigma),
                        up,
                    )
                    .expect("settled");
                assert_eq!(
                    mine, limit.count,
                    "bend {bend}, flipped {flipped}, σ {sigma}"
                );
                // `±d + 2^-8·T₂ + 2^-16·T₃ + 2^-24·T₄`, times `2^-28`.
                let v = d * f64::from(sigma) + t[0] / 256.0 + t[1] / 65536.0 + t[2] / 16777216.0;
                let scaled = oracle
                    .ray(
                        &Shift::NONE,
                        q.to_array(),
                        &Shift::always(v.to_array(), -28),
                        up,
                    )
                    .expect("settled");
                assert_eq!(
                    mine, scaled.count,
                    "bend {bend}, flipped {flipped}, σ {sigma}"
                );
            }
        }
    }
    // The arc bulging either way: the same translation enters one fold.
    assert_eq!(entered, 4);
}

/// A tile through `c` (at `(½, ½)`) not bending along the column there:
/// `(s, t) ↦ c + (t − s²)·r̂ + (s·t − t²)·ν + s·ê`, `s = u − ½`, `t = v −
/// ½`, exactly (its Bernstein net). The column of `c` meets it there at a
/// root of multiplicity three (`s³ − s⁴`), tangent to it (`x_u = ê`,
/// `x_v = r̂`) with no bend (`x_uu = −2r̂`, along the tangent plane).
fn flat_fold_tile(c: DVec3) -> TileNet {
    let s = [-0.5, 0.0, 0.5];
    let s2 = [0.25, -0.25, 0.25];
    std::array::from_fn(|k| {
        let (i, j) = (k / 3, k % 3);
        let x = c + R_HAT * (s[j] - s2[i]) + NU * (s[i] * s[j] - s2[j]) + UP * s[i];
        [x.x, x.y, x.z, 1.0]
    })
}

#[test]
fn a_column_tangent_to_a_tile_not_bending_there_forms_no_hit() {
    let c = DVec3::new(0.25, -0.5, 0.125);
    for flipped in [false, true] {
        let cf = fold_face(flat_fold_tile(c), flipped);
        let face = cf.hit_face();
        for d in [R_HAT, -R_HAT, NU, R_HAT + NU] {
            let rates = rates_a(Sign::Pos, d);
            let motion: Motion = std::array::from_fn(|k| rate_rational(&rates[k]).map(|x| -x));
            assert_eq!(
                face_hits(&face, c.to_array(), &motion),
                Err(HitError::Fold([0, 0])),
                "flipped {flipped}, d {d:?}"
            );
        }
        // A column beside it crosses the tile once (`s³ − s⁴ = k`).
        for k in [1.0 / 4096.0, -1.0 / 4096.0] {
            let q = c + NU * k;
            let hits = face_hits(&face, q.to_array(), &motion_of(Operand::A)).expect("a hit");
            assert_eq!(hits.len(), 1, "{hits:#?}");
        }
    }
}

#[test]
fn a_column_tangent_to_a_tile_inflecting_off_q_is_one_hit() {
    // `q` moved along `ê` off `c`: the same column, tangent to the tile
    // at `c` (a root of multiplicity three, the tile inflecting along
    // `ê`), now off `q`'s height. It crosses the tile once there, facing
    // as the tile does on either side; the upward count is the oracle's.
    let up = [UP.x, UP.y, UP.z];
    let c = DVec3::new(0.25, -0.5, 0.125);
    for flipped in [false, true] {
        let cf = fold_face(flat_fold_tile(c), flipped);
        let face = cf.hit_face();
        let oracle = cf.oracle();
        for k in [1.0 / 4096.0, -1.0 / 4096.0, 1.0 / 16.0, -1.0 / 16.0] {
            let q = c + UP * k;
            let hits = face_hits(&face, q.to_array(), &motion_of(Operand::A)).expect("a hit");
            assert_eq!(hits.len(), 1, "flipped {flipped}, k {k}: {hits:#?}");
            let h = &hits[0];
            assert_eq!(h.sheet, Sheet::Tile([0, 0]));
            let want = if k > 0.0 {
                Ordering::Less
            } else {
                Ordering::Greater
            };
            assert_eq!(h.height, want);
            // `(x_u × x_v)·ê ∝ 4s³ − 3s²` along the column's curve on the
            // tile, `s = u − ½`: below zero on either side of `c`.
            let down = if flipped { Sign::Pos } else { Sign::Neg };
            assert_eq!(h.facing, Some(down), "flipped {flipped}, k {k}");
            let mine = count_up(&hits).expect("heights");
            // The ray tangent to the tile: the oracle's count of the
            // column moved off it by `2^e` across the tile (along `ν`, its
            // normal at `c`), its hits there within about `2^(e/3)` of `c`
            // along `ê`, off `q`'s height.
            let e = if k.abs() > 1.0 / 64.0 { -30 } else { -40 };
            for d in [NU, -NU, R_HAT + NU, R_HAT - NU] {
                let o = oracle
                    .ray(
                        &Shift::NONE,
                        q.to_array(),
                        &Shift::always(d.to_array(), e),
                        up,
                    )
                    .unwrap_or_else(|e2| panic!("flipped {flipped}, k {k}, d {d:?}: {e2:?}"));
                assert_eq!(mine, o.count, "flipped {flipped}, k {k}, d {d:?}");
            }
        }
    }
}

#[test]
fn the_local_degree_at_a_transversal_hit_is_its_facing() {
    // Columns over cylinder and sphere tiles: at every root where the
    // column crosses the tile, the winding of the column's equations
    // round it is the sign of their Jacobian there.
    let mut rng = Rng::new(0x1d9);
    let [a, b, _] = frame_dirs();
    let mut checked = 0;
    for n in 0..24 {
        let net = if n % 2 == 0 {
            cylinder(&mut rng)
        } else {
            sphere(&mut rng)
        };
        for _ in 0..6 {
            let q = net_at(&net, rng.range(0.0, 1.0), rng.range(0.0, 1.0))
                + rng.direction() * rng.range(0.0, 0.5);
            let qv = q.to_array().map(rat);
            let (f1, f2) = (
                bern2(&net_coefs(&net, &a, &qv)),
                bern2(&net_coefs(&net, &b, &qv)),
            );
            let Some(roots) =
                tile_roots(&f1, &f2, [0, 0], false).or_else(|| tile_roots(&f1, &f2, [0, 0], true))
            else {
                continue;
            };
            let jac = sub2(&mul2(&du2(&f1), &dv2(&f2)), &mul2(&dv2(&f1), &du2(&f2)));
            let (zero, one) = (Rational::ZERO, Rational::ONE);
            for (root, _) in roots {
                // The tile's roots only: the box holds the other
                // coordinate's `[0, 1]`.
                if (0..2).any(|k| {
                    root.coord_cmp(k, &zero) == Ordering::Less
                        || root.coord_cmp(k, &one) == Ordering::Greater
                }) {
                    continue;
                }
                let want = match sign_at_tile(&jac, &root).expect("a sign") {
                    Ordering::Greater => 1,
                    Ordering::Less => -1,
                    Ordering::Equal => continue,
                };
                assert_eq!(local_degree(&f1, &f2, &root), Some(want), "q {q:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 40, "{checked}");
}

#[test]
fn a_fold_on_a_trim_at_q_forms_no_hit() {
    // The face trimmed at the silhouette ruling, `u = ½`: the fold the
    // moved column enters lies on the trim at `q`.
    let c = DVec3::new(0.25, -0.5, 0.125);
    let net = fold_tile(1.0);
    let q = c + R_HAT * (2.0 / 64.0) + (R_HAT + NU) * (1.0 / 1024.0);
    let cf = fold_face_over(net, false, 0.5);
    let face = cf.hit_face();
    let rates = rates_a(Sign::Pos, -R_HAT);
    let motion: Motion = std::array::from_fn(|k| rate_rational(&rates[k]).map(|x| -x));
    assert_eq!(
        face_hits(&face, q.to_array(), &motion),
        Err(HitError::Fold([0, 0]))
    );
}

#[test]
fn a_column_touching_a_cylinder_off_q_cancels() {
    // Above or below the silhouette point: the touch's two hits cancel,
    // none listed, whatever the motion.
    let net = fold_tile(1.0);
    let cf = fold_face(net, false);
    let face = cf.hit_face();
    let c = DVec3::new(0.25, -0.5, 0.125);
    for dz in [1.0 / 64.0, -1.0 / 64.0] {
        let q = c + R_HAT * (2.0 / 64.0) + (R_HAT + NU) * (1.0 / 1024.0) + UP * dz;
        let hits = face_hits(&face, q.to_array(), &motion_of(Operand::A)).expect("hits");
        assert!(hits.is_empty(), "{hits:#?}");
    }
}

/// The face's normal at `(u, v)` by central differences, in `f64`.
fn normal_at(net: &TileNet, u: f64, v: f64) -> DVec3 {
    let h = 1e-6;
    let du = (net_at(net, u + h, v) - net_at(net, u - h, v)) / (2.0 * h);
    let dv = (net_at(net, u, v + h) - net_at(net, u, v - h)) / (2.0 * h);
    du.cross(dv)
}

#[test]
fn germ_rows_are_the_tangent_plane_at_the_point() {
    // `q` at an edge curve's start (on the curve exactly) and at the
    // face's vertex there: the row is the face's tangent plane at the
    // p-curve's point, against `q` moving along `d`.
    let mut rng = Rng::new(0x6e7);
    let mut checked = 0;
    for _ in 0..10 {
        let net = cylinder(&mut rng);
        let cf = CurvedFace::random(&mut rng, net, 1.0 / 16.0, 1.0 / 32.0);
        let mut face = cf.hit_face();
        for (k, t) in face.trims.iter_mut().enumerate() {
            t.ends = [VertId(10 + k as u32), VertId(20 + k as u32)];
        }
        for k in 0..4 {
            let c = cf.curves[k][0];
            let q = [c[0], c[1], c[2]];
            let uv = cf.pspans[k][0];
            let n = normal_at(&net, uv[0], uv[1]);
            for _ in 0..4 {
                let d = rng.direction();
                let rates = rates_a(Sign::Pos, d);
                let motion: Motion = std::array::from_fn(|i| rate_rational(&rates[i]).map(|x| -x));
                let (a, f) = (n.dot(-d), n.dot(UP));
                if a.abs() < 1e-6 * n.length() || f.abs() < 1e-6 * n.length() {
                    continue;
                }
                let want = if a * f > 0.0 {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
                for at in [
                    GermAt::Curve(EdgeId(k as u32)),
                    GermAt::Vertex(VertId(10 + k as u32)),
                ] {
                    let ord = face.germ_orders(at, q, &motion).expect("a row");
                    assert_eq!(ord[0], Ok(want), "{at:?}");
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "{checked}");
}
