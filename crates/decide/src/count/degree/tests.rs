//! Box degrees of random faces against the exact winding: faces of 3 × 2
//! biquadratic tiles with random weights, their outer boundary straight
//! (collinear control points) so its shadow is a quadrilateral whose
//! winding about the query is computed exactly on rationals, their inner
//! control points (tile lines included) thrown about so the shadow folds;
//! random partitions into boxes, split off the query, sum to it, flipped
//! faces negated. Also: additivity under further splits and of side turns
//! under cuts, the same degree from every ray and level, polygons by
//! segments against the exact winding, and sides, tile lines and corners
//! through the query reported as such.

use std::collections::VecDeque;
use std::ops::Range;

use malachite::Rational as Q;
use varde_testkit::rng::Rng;

use super::*;
use crate::bezier::Dyadic;
use crate::count::boxes::{Extent, Trim, split};
use crate::ids::FaceId;
use crate::num::dd::Dd;

type Patch = HomPatch<Bounded<f64>>;

const TILES: [Range<i64>; 2] = [0..3, 0..2];
/// Control points per row of the whole grid: two per tile and one more.
const NU_PTS: usize = 7;
const NV_PTS: usize = 5;

fn first() -> Bounded<f64> {
    Bounded::exact(0.0)
}

fn dd() -> Bounded<Dd> {
    Bounded::exact(Dd::from(0.0))
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

/// A random integer in `[−k, k]`, as a float.
fn int(rng: &mut Rng, k: u64) -> f64 {
    (rng.next_u64() % (2 * k + 1)) as f64 - k as f64
}

/// A face's control grid, affine points with weights, `[u][v]`, and its
/// four corners `[[c00, c01], [c10, c11]]` (by `u`, then `v`).
struct Surface {
    grid: Vec<Vec<[f64; 4]>>,
    corners: [[[f64; 3]; 2]; 2],
}

/// Dyadic positions of the boundary control points along an edge with
/// `n` points: increasing, so the boundary runs straight from corner to
/// corner, exact in products with the corners' integers.
fn along(n: usize) -> Vec<f64> {
    match n {
        NU_PTS => [0.0, 1.0, 3.0, 4.0, 5.0, 7.0, 8.0]
            .map(|k| k / 8.0)
            .to_vec(),
        NV_PTS => [0.0, 1.0, 2.0, 3.0, 4.0].map(|k| k / 4.0).to_vec(),
        _ => unreachable!(),
    }
}

/// A random surface over the tiles: corners at integers, the outer
/// boundary's control points on the segments between them, the inner
/// ones moved by up to `wild` in `x` and `y` (and 8 in `z`) from the
/// bilinear position, every weight in `[0.5, 2]`.
fn surface(rng: &mut Rng, wild: f64) -> Surface {
    let mut corners = [[[0.0; 3]; 2]; 2];
    for (a, row) in corners.iter_mut().enumerate() {
        for (b, c) in row.iter_mut().enumerate() {
            *c = [
                30.0 * a as f64 + int(rng, 6),
                20.0 * b as f64 + int(rng, 4),
                int(rng, 8),
            ];
        }
    }
    let (su, sv) = (along(NU_PTS), along(NV_PTS));
    let lerp = |p: [f64; 3], r: [f64; 3], t: f64| std::array::from_fn(|k| p[k] + t * (r[k] - p[k]));
    let mut grid = vec![vec![[0.0; 4]; NV_PTS]; NU_PTS];
    for i in 0..NU_PTS {
        for j in 0..NV_PTS {
            let (s, t) = (su[i], sv[j]);
            let c = &corners;
            let p: [f64; 3] = if j == 0 || j == NV_PTS - 1 {
                let b = usize::from(j != 0);
                lerp(c[0][b], c[1][b], s)
            } else if i == 0 || i == NU_PTS - 1 {
                let a = usize::from(i != 0);
                lerp(c[a][0], c[a][1], t)
            } else {
                let x = lerp(lerp(c[0][0], c[1][0], s), lerp(c[0][1], c[1][1], s), t);
                [
                    x[0] + rng.range(-wild, wild),
                    x[1] + rng.range(-wild, wild),
                    x[2] + rng.range(-8.0, 8.0),
                ]
            };
            grid[i][j] = [p[0], p[1], p[2], rng.range(0.5, 2.0)];
        }
    }
    Surface { grid, corners }
}

impl Surface {
    /// Tile `(i, j)`'s net, row-major in `u`.
    fn net(&self, [i, j]: [i64; 2]) -> Vec<[f64; 4]> {
        let mut n = Vec::new();
        for a in 0..3 {
            for b in 0..3 {
                n.push(self.grid[2 * i as usize + a][2 * j as usize + b]);
            }
        }
        n
    }

    /// The tiles' patches at `like`'s level, in the face's tile order.
    fn patches<S: Scalar>(&self, like: &S) -> Vec<HomPatch<S>> {
        let mut out = Vec::new();
        for i in TILES[0].clone() {
            for j in TILES[1].clone() {
                out.push(HomPatch::from_affine(2, 2, self.net([i, j]), like).unwrap());
            }
        }
        out
    }

    /// The exact winding of the outer boundary's shadow about `p`'s, `None`
    /// when `p`'s shadow lies on it.
    fn winding(&self, p: [f64; 3]) -> Option<i64> {
        let c = &self.corners;
        let ring = [c[0][0], c[1][0], c[1][1], c[0][1]].map(exact_shadow);
        polygon_winding(&ring, &exact_shadow(p))
    }
}

/// `(r̂·x, ν·x)` exactly.
fn exact_shadow(x: [f64; 3]) -> [Q; 2] {
    let d = |a: glam::DVec3| q(a.x) * q(x[0]) + q(a.y) * q(x[1]) + q(a.z) * q(x[2]);
    [d(R_HAT), d(NU)]
}

/// The winding of the closed polygon `ring` about `p`, exactly; `None`
/// when `p` lies on it.
fn polygon_winding(ring: &[[Q; 2]], p: &[Q; 2]) -> Option<i64> {
    let mut w = 0;
    for k in 0..ring.len() {
        let (a, b) = (&ring[k], &ring[(k + 1) % ring.len()]);
        let cross = (&b[0] - &a[0]) * (&p[1] - &a[1]) - (&b[1] - &a[1]) * (&p[0] - &a[0]);
        let within = |s: &Q, t: &Q, x: &Q| (s <= x && x <= t) || (t <= x && x <= s);
        if cross == 0 && within(&a[0], &b[0], &p[0]) && within(&a[1], &b[1], &p[1]) {
            return None;
        }
        if a[1] <= p[1] && p[1] < b[1] && cross > 0 {
            w += 1;
        } else if b[1] <= p[1] && p[1] < a[1] && cross < 0 {
            w -= 1;
        }
    }
    Some(w)
}

fn face_uv<'a>(trims: &'a [Trim<'a>], flipped: bool) -> FaceUv<'a> {
    FaceUv::new(Operand::B, FaceId(3), TILES, [1.0, 1.0], trims, flipped).unwrap()
}

fn patch_of<'p, P>(ps: &'p [P], b: &BoxKey) -> &'p P {
    let [i, j] = b.tile();
    let nv = (TILES[1].end - TILES[1].start) as usize;
    &ps[(i - TILES[0].start) as usize * nv + (j - TILES[1].start) as usize]
}

/// The patch's point at local `(s, t)`, rounded.
fn point_on(p: &Patch, st: [f64; 2]) -> [f64; 3] {
    let x = p.eval(&Bounded::exact(st[0]), &Bounded::exact(st[1]));
    x.map(|c| {
        let [lo, hi] = c.enclose();
        lo + (hi - lo) * 0.5
    })
}

/// A random partition of the face's tiles: each box split (off `qs`)
/// with probability `p` while shallower than `depth`, always while its
/// hull holds the query and shallower than `near`.
fn partition(
    rng: &mut Rng,
    face: &FaceUv<'_>,
    ps: &[Patch],
    qs: &QShadow,
    p: f64,
    depth: u32,
    near: u32,
) -> Vec<BoxKey> {
    let mut queue: VecDeque<BoxKey> = face.tile_boxes().collect();
    let mut leaves = Vec::new();
    while let Some(b) = queue.pop_front() {
        let patch = patch_of(ps, &b);
        let close = !hull_off(&sub_patch(patch, &b), qs) && b.depth() < near;
        if close || (b.depth() < depth && rng.unit() < p) {
            queue.extend(split(face, &b, patch, Some(qs)).unwrap().halves);
        } else {
            leaves.push(b);
        }
        assert!(leaves.len() + queue.len() < 100_000, "refinement runs away");
    }
    leaves
}

/// The sum of the leaves' degrees, `None` where one isn't certified (a
/// side through the query, or unsettled).
fn total<S: Scalar>(
    face: &FaceUv<'_>,
    ps: &[HomPatch<S>],
    leaves: &[BoxKey],
    qs: &QShadow,
) -> Option<i64> {
    let mut sum = 0;
    for b in leaves {
        sum += box_degree(face, b, patch_of(ps, b), qs).ok()?.degree;
    }
    Some(sum)
}

/// A random query point over the face's shadow and a little beyond.
fn query(rng: &mut Rng) -> [f64; 3] {
    [
        rng.range(-10.0, 40.0),
        rng.range(-8.0, 28.0),
        rng.range(-10.0, 10.0),
    ]
}

#[test]
fn degrees_sum_to_the_exact_winding() {
    let trims: [Trim<'_>; 0] = [];
    let (mut used, mut tried, mut inside) = (0, 0, 0);
    for seed in varde_testing::seeds(24, 400) {
        let mut rng = Rng::new(seed);
        let wild = [0.0, 3.0, 12.0][(seed % 3) as usize];
        let s = surface(&mut rng, wild);
        let ps = s.patches(&first());
        let flipped = seed % 2 == 1;
        let face = face_uv(&trims, flipped);
        for _ in 0..4 {
            let qp = query(&mut rng);
            let Some(w) = s.winding(qp) else { continue };
            let want = if flipped { -w } else { w };
            let qs = QShadow::of(qp).unwrap();
            for _ in 0..2 {
                tried += 1;
                let leaves = partition(&mut rng, &face, &ps, &qs, 0.5, 5, 7);
                let Some(got) = total(&face, &ps, &leaves, &qs) else {
                    continue;
                };
                used += 1;
                inside += usize::from(want != 0);
                assert_eq!(
                    got,
                    want,
                    "seed {seed}, query {qp:?}, {} boxes",
                    leaves.len()
                );
            }
        }
    }
    assert!(
        used * 10 >= tried * 9,
        "{used} of {tried} partitions certified"
    );
    assert!(inside * 4 >= used, "{inside} of {used} queries inside");
}

/// A box's degree is the sum of its halves', whichever split, and the
/// same from every ray that settles it and at every level.
#[test]
fn degrees_add_up_and_agree() {
    let trims: [Trim<'_>; 0] = [];
    let (mut checked, mut rays_compared, mut levels) = (0, 0, 0);
    for seed in varde_testing::seeds(16, 256) {
        let mut rng = Rng::new(seed);
        let s = surface(&mut rng, 12.0);
        let (ps, pd, pq) = (
            s.patches(&first()),
            s.patches(&dd()),
            s.patches(&Q::from(0)),
        );
        let face = face_uv(&trims, seed % 2 == 0);
        let qp = query(&mut rng);
        let qs = QShadow::of(qp).unwrap();
        let mut boxes = partition(&mut rng, &face, &ps, &qs, 0.6, 4, 5);
        // The boxes under the query first: the rest have degree 0 at once.
        boxes.sort_by_key(|b| hull_off(&sub_patch(patch_of(&ps, b), b), &qs));
        for b in boxes.iter().take(24) {
            let patch = patch_of(&ps, b);
            let Ok(d) = box_degree(&face, b, patch, &qs) else {
                continue;
            };
            // At the higher levels and on rationals.
            for other in [
                box_degree(&face, b, patch_of(&pd, b), &qs).map(|d| d.degree),
                box_degree(&face, b, patch_of(&pq, b), &qs).map(|d| d.degree),
            ]
            .into_iter()
            .flatten()
            {
                levels += 1;
                assert_eq!(other, d.degree, "seed {seed}: {b:?} by level");
            }
            // Every ray that settles all four sides.
            if d.ray.is_some() {
                for ray in Ray::ALL {
                    let mut n = 0;
                    let laps: Result<Vec<_>, _> = Side::CCW
                        .iter()
                        .map(|&side| side_turn(b, patch, side, &qs, ray, &mut n).map(|t| t.laps))
                        .collect();
                    if let Ok(l) = laps {
                        rays_compared += 1;
                        assert_eq!(
                            face.inside_winding() * l.iter().sum::<i64>(),
                            d.degree,
                            "seed {seed}: {ray:?}"
                        );
                    }
                }
            }
            // Its halves, split off the query.
            let halves = split(&face, b, patch, Some(&qs)).unwrap().halves;
            let parts: Result<Vec<_>, _> = halves
                .iter()
                .map(|h| box_degree(&face, h, patch, &qs))
                .collect();
            if let Ok(parts) = parts {
                checked += 1;
                assert_eq!(
                    parts[0].degree + parts[1].degree,
                    d.degree,
                    "seed {seed}: {b:?}"
                );
            }
        }
    }
    assert!(checked > 100, "{checked} splits checked");
    assert!(rays_compared > 100, "{rays_compared} rays compared");
    assert!(levels > 100, "{levels} levels compared");
}

/// A side's turn is the sum of its two parts' at a cut, about every ray.
#[test]
fn side_turns_add_up_under_cuts() {
    let mut checked = 0;
    for seed in varde_testing::seeds(16, 256) {
        let mut rng = Rng::new(seed);
        let s = surface(&mut rng, 12.0);
        let ps = s.patches(&first());
        let qs = QShadow::of(query(&mut rng)).unwrap();
        for _ in 0..8 {
            let patch = &ps[(rng.next_u64() % ps.len() as u64) as usize];
            let dir = if rng.next_u64().is_multiple_of(2) {
                Dir::U
            } else {
                Dir::V
            };
            let at = Dyadic::new(rng.next_u64() % 17, 4).unwrap();
            let a = Dyadic::new(rng.next_u64() % 8, 4).unwrap();
            let c = Dyadic::new(9 + rng.next_u64() % 8, 4).unwrap();
            let mid = Dyadic::new(8, 4).unwrap();
            let whole = Extent::new(a, c).unwrap();
            let parts = [Extent::new(a, mid).unwrap(), Extent::new(mid, c).unwrap()];
            for ray in Ray::ALL {
                let mut n = 0;
                let t = |e: Extent, n: &mut u64| iso_turn(patch, dir, at, e, &qs, ray, n);
                let (Ok(w), Ok(l), Ok(r)) =
                    (t(whole, &mut n), t(parts[0], &mut n), t(parts[1], &mut n))
                else {
                    continue;
                };
                checked += 1;
                let joined = l.then(r);
                assert_eq!(joined.laps, w.laps, "seed {seed}: {ray:?}");
                // Ends seen from either are the same facts.
                for (x, y) in [(w.start, l.start), (w.end, r.end), (l.end, r.start)] {
                    assert!(!matches!(
                        (x, y),
                        (End::Left, End::Right) | (End::Right, End::Left)
                    ));
                }
                // Backwards, the same increment negated.
                assert_eq!(joined.reversed().laps, -w.laps);
            }
        }
    }
    assert!(checked > 500, "{checked} cuts checked");
}

/// A degree-1 span from `a` to `b` in space, at the first level.
fn segment(a: [f64; 3], b: [f64; 3]) -> HomCurve<Bounded<f64>, 3> {
    HomCurve::from_affine([(a, 1.0), (b, 1.0)], &first()).unwrap()
}

/// Random polygons by their segments, against every ray: the laps sum to
/// the exact winding, and a segment through the query isn't certified.
#[test]
fn polygon_turns_match_the_exact_winding() {
    let (mut checked, mut tried) = (0, 0);
    for seed in varde_testing::seeds(32, 512) {
        let mut rng = Rng::new(seed);
        let n = 3 + (rng.next_u64() % 6) as usize;
        let ring: Vec<[f64; 3]> = (0..n)
            .map(|_| [int(&mut rng, 4), int(&mut rng, 4), int(&mut rng, 4)])
            .collect();
        let qp = [int(&mut rng, 3), int(&mut rng, 3) * 0.5 + 0.25, 0.0];
        let exact: Vec<_> = ring.iter().map(|&x| exact_shadow(x)).collect();
        let qs = QShadow::of(qp).unwrap();
        let Some(want) = polygon_winding(&exact, &exact_shadow(qp)) else {
            continue;
        };
        for ray in Ray::ALL {
            tried += 1;
            let mut pieces = 0;
            let turns: Result<Vec<Turn>, _> = (0..n)
                .map(|k| curve_turn(&segment(ring[k], ring[(k + 1) % n]), &qs, ray, &mut pieces))
                .collect();
            if let Ok(t) = turns {
                checked += 1;
                let all = t.iter().copied().reduce(Turn::then).unwrap();
                assert_eq!(all.laps, want, "seed {seed}: {ray:?}");
            }
        }
    }
    assert!(
        checked * 3 > tried * 2,
        "{checked} of {tried} polygons checked"
    );
    // Through the query.
    let mut n = 0;
    let qs = QShadow::of([0.5, 0.25, 0.0]).unwrap();
    for ray in Ray::ALL {
        let through = segment([0.0, 0.0, 0.0], [1.0, 0.5, 0.0]);
        assert_eq!(curve_turn(&through, &qs, ray, &mut n), Err(TurnError::Off));
    }
}

/// Laps of known curves: a square round the query once each way, a ray
/// crossed and crossed back, and an end on the ray's line.
#[test]
fn known_turns() {
    let qs = QShadow::of([0.0; 3]).unwrap();
    // Corners of a square round the origin's shadow, counter-clockwise in
    // the shadow coordinates: solve for points with given (r̂·x, ν·x) on
    // z = 0 (r̂·x = 16x, ν·x = −3x + 514y).
    let at = |r: f64, n: f64| {
        let x = r / 16.0;
        [x, (n + 3.0 * x) / 514.0, 0.0]
    };
    let sq = [at(1.0, -2.0), at(1.0, 2.0), at(-1.0, 2.0), at(-1.0, -2.0)];
    let mut n = 0;
    for ray in Ray::ALL {
        let ccw: i64 = (0..4)
            .map(|k| {
                curve_turn(&segment(sq[k], sq[(k + 1) % 4]), &qs, ray, &mut n)
                    .unwrap()
                    .laps
            })
            .sum();
        let cw: i64 = (0..4)
            .map(|k| {
                curve_turn(&segment(sq[(k + 1) % 4], sq[k]), &qs, ray, &mut n)
                    .unwrap()
                    .laps
            })
            .sum();
        assert_eq!((ccw, cw), (1, -1), "{ray:?}");
    }
    // The right side crosses `+r̂` once, counter-clockwise; `−r̂` sees it
    // behind the query.
    let right = segment(sq[0], sq[1]);
    let t = curve_turn(&right, &qs, Ray::PLUS_R, &mut n).unwrap();
    assert_eq!(
        t,
        Turn {
            laps: 1,
            start: End::Right,
            end: End::Left
        }
    );
    let t = curve_turn(&right, &qs, Ray::MINUS_R, &mut n).unwrap();
    assert_eq!(
        t,
        Turn {
            laps: 0,
            start: End::Behind,
            end: End::Behind
        }
    );
    // Up across `+r̂` and back down: no net crossing.
    let back = curve_turn(&right.reversed(), &qs, Ray::PLUS_R, &mut n).unwrap();
    assert_eq!(
        back,
        Turn {
            laps: -1,
            start: End::Left,
            end: End::Right
        }
    );
    let up = curve_turn(&right, &qs, Ray::PLUS_R, &mut n).unwrap();
    assert_eq!(up.then(back).laps, 0);
    // An end exactly on `+r̂`'s line ahead (`r̂` itself, `ν·r̂ = 0`): not
    // settled for that ray; for `+ν` the segment is right of the line.
    let on = segment([16.0, 0.0, -1.0], [16.0, 1.0, -1.0]);
    assert_eq!(
        curve_turn(&on, &qs, Ray::PLUS_R, &mut n),
        Err(TurnError::End)
    );
    assert_eq!(
        curve_turn(&on, &qs, Ray::PLUS_NU, &mut n),
        Ok(Turn {
            laps: 0,
            start: End::Right,
            end: End::Right
        })
    );
}

/// Sides through the query: a tile line, a dyadic line, a box corner;
/// the box's degree names the side, the hull fast path is never taken.
#[test]
fn sides_through_the_query_are_reported() {
    let trims: [Trim<'_>; 0] = [];
    for seed in varde_testing::seeds(16, 128) {
        let mut rng = Rng::new(seed);
        let s = surface(&mut rng, 3.0);
        let ps = s.patches(&first());
        let face = face_uv(&trims, false);
        let tiles: Vec<BoxKey> = face.tile_boxes().collect();
        // Tile (0, 0)'s right side is tile (1, 0)'s left: the tile line
        // u = 1.
        let qp = point_on(&ps[0], [1.0, rng.range(0.2, 0.8)]);
        let qs = QShadow::of(qp).unwrap();
        let right_of = tiles.iter().find(|b| b.tile() == [1, 0]).unwrap();
        assert_eq!(
            box_degree(&face, &tiles[0], &ps[0], &qs),
            Err(DegreeError::Through(Side::Right))
        );
        assert_eq!(
            box_degree(&face, right_of, patch_of(&ps, right_of), &qs),
            Err(DegreeError::Through(Side::Left))
        );
        assert!(Side::Right.on_tile_line(&tiles[0]));
        // The dyadic line v = 1/2 of tile (0, 0), not moved off it.
        let half = Dyadic::new(1, 1).unwrap();
        let qp = point_on(&ps[0], [rng.range(0.2, 0.8), 0.5]);
        let qs = QShadow::of(qp).unwrap();
        let [lo, hi] = tiles[0].split_at(Dir::V, half).unwrap();
        assert_eq!(
            box_degree(&face, &lo, &ps[0], &qs),
            Err(DegreeError::Through(Side::Top))
        );
        assert_eq!(
            box_degree(&face, &hi, &ps[0], &qs),
            Err(DegreeError::Through(Side::Bottom))
        );
        assert!(!Side::Top.on_tile_line(&lo));
        // The corner (1/2, 1/2) of the box [0, 1/2]²: its right side, the
        // first counter-clockwise through it.
        let qs = QShadow::of(point_on(&ps[0], [0.5, 0.5])).unwrap();
        let [quarter, _] = lo.split_at(Dir::U, half).unwrap();
        assert_eq!(
            box_degree(&face, &quarter, &ps[0], &qs),
            Err(DegreeError::Through(Side::Right))
        );
    }
}

#[test]
fn refuses_foreign_boxes() {
    let trims: [Trim<'_>; 0] = [];
    let face = face_uv(&trims, false);
    let ps = surface(&mut Rng::new(1), 0.0).patches(&first());
    let qs = QShadow::of([0.0; 3]).unwrap();
    let other = BoxKey::new(Operand::A, FaceId(3), [0, 0], Extent::WHOLE, Extent::WHOLE);
    assert_eq!(
        box_degree(&face, &other, &ps[0], &qs),
        Err(DegreeError::Box(BoxError::Foreign))
    );
    assert_eq!(Ray::new(0, 0), None);
    assert_eq!(Ray::new(2, 0), None);
    assert_eq!(Ray::new(-1, 1), Some(Ray::ALL[5]));
    assert_eq!(Ray::of_query(Operand::A), Ray::MINUS_R);
}

/// A side that starts on the ray's line ahead of the query and then runs
/// through it is through the query, whatever the end: the end left
/// unsettled must not hide it (else the box only ever escalates, never
/// reaching the side's record).
#[test]
fn a_side_through_the_query_from_the_ray_is_reported() {
    // Multiples of `r̂ = (16, 0, −1)` have `ν·x = 0`: the bottom side runs
    // along `+r̂`'s line from ahead of the origin through it to behind;
    // the top is `ν`-shifted (`y = 1`).
    let net = [
        [32.0, 0.0, -2.0, 1.0],
        [32.0, 1.0, -2.0, 1.0],
        [-32.0, 0.0, 2.0, 1.0],
        [-32.0, 1.0, 2.0, 1.0],
    ];
    let patch = Patch::from_affine(1, 1, net, &first()).unwrap();
    let qs = QShadow::of([0.0; 3]).unwrap();
    let mut n = 0;
    let bottom = segment([32.0, 0.0, -2.0], [-32.0, 0.0, 2.0]);
    for ray in Ray::ALL {
        assert_eq!(
            curve_turn(&bottom, &qs, ray, &mut n),
            Err(TurnError::Off),
            "{ray:?}"
        );
    }
    let trims: [Trim<'_>; 0] = [];
    let face = face_uv(&trims, false);
    let b = face.tile_boxes().next().unwrap();
    assert_eq!(
        box_degree(&face, &b, &patch, &qs),
        Err(DegreeError::Through(Side::Bottom))
    );
}
