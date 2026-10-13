//! Boxes of random faces (rectangles and discs of circle arcs, with
//! holes, flipped faces, coedges stored against their edges, trims on
//! tile lines and dyadic lines) refined to a depth: every core and
//! outside box is clear of every trim by an exact algebraic test on the
//! stored spans and lies inside or outside the shape as its class says;
//! every box stays in one tile and the boxes of a tile cover it once;
//! splits move off a query point's shadow; the same boxes and classes
//! come from every route, trim order and level; bad input is refused;
//! boxes refined by role, and tubes round curved spans, on the weighted
//! curved surface.

use std::cmp::Ordering;
use std::collections::VecDeque;

use malachite::Rational as Q;
use varde_testkit::rng::Rng;

use super::*;
use crate::num::dd::Dd;

type Patch = HomPatch<Bounded<f64>>;

const TILES: [Range<i64>; 2] = [0..3, 0..2];

fn first() -> Bounded<f64> {
    Bounded::exact(0.0)
}

fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

/// The height of the test surface over `(x, y)`.
fn height(x: f64, y: f64) -> f64 {
    0.1 * x * y - 0.05 * x * x + 0.2 * y
}

/// Tile `(i, j)`'s net: a graph over the tile, the weight of each control
/// point `weight` of its global position in half tiles.
fn net(i: i64, j: i64, weight: &impl Fn(usize, usize) -> f64) -> Vec<[f64; 4]> {
    let mut n = Vec::new();
    for a in 0..3 {
        for b in 0..3 {
            let (x, y) = (i as f64 + a as f64 * 0.5, j as f64 + b as f64 * 0.5);
            let at = |t: i64, r: &Range<i64>, k: usize| (2 * (t - r.start)) as usize + k;
            let w = weight(at(i, &TILES[0], a), at(j, &TILES[1], b));
            n.push([x, y, height(x, y), w]);
        }
    }
    n
}

/// The tiles' patches, row-major in `u`, at the first level: unit weights,
/// or weights drawn per control point of the whole grid, so that
/// neighbouring tiles share their boundary rows, points and weights, and
/// the surface is one continuous surface across tile lines.
fn patches(rng: Option<&mut Rng>) -> Vec<Patch> {
    let n = |r: &Range<i64>| (2 * (r.end - r.start) + 1) as usize;
    let (nu, nv) = (n(&TILES[0]), n(&TILES[1]));
    let grid: Vec<f64> = match rng {
        Some(r) => (0..nu * nv).map(|_| r.range(0.5, 2.0)).collect(),
        None => vec![1.0; nu * nv],
    };
    let weight = |u: usize, v: usize| grid[u * nv + v];
    let mut out = Vec::new();
    for i in TILES[0].clone() {
        for j in TILES[1].clone() {
            out.push(Patch::from_affine(2, 2, net(i, j, &weight), &first()).unwrap());
        }
    }
    out
}

fn patch_of<'p, P>(ps: &'p [P], b: &BoxKey) -> &'p P {
    let [i, j] = b.tile();
    let nv = (TILES[1].end - TILES[1].start) as usize;
    &ps[(i - TILES[0].start) as usize * nv + (j - TILES[1].start) as usize]
}

/// The distance between two points.
fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1]];
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}

/// A shape of a test face, in global UV.
#[derive(Clone, Copy, Debug)]
enum Shape {
    /// `[u0, v0, u1, v1]`.
    Rect([f64; 4]),
    /// Centre and radius.
    Disc([f64; 2], f64),
}

impl Shape {
    /// The signed distance, positive inside (a rectangle's by its sides).
    fn depth(self, p: [f64; 2]) -> f64 {
        match self {
            Shape::Rect([u0, v0, u1, v1]) => {
                let d = [p[0] - u0, u1 - p[0], p[1] - v0, v1 - p[1]];
                d.into_iter()
                    .fold(f64::INFINITY, |m, x| if x < m { x } else { m })
            }
            Shape::Disc(c, r) => r - dist(p, c),
        }
    }

    /// The bounding box `[u0, v0, u1, v1]`.
    fn bounds(self) -> [f64; 4] {
        match self {
            Shape::Rect(r) => r,
            Shape::Disc(c, r) => [c[0] - r, c[1] - r, c[0] + r, c[1] + r],
        }
    }

    /// The counter-clockwise loop: per coedge its spans.
    fn ccw(self) -> Vec<Vec<UvSpan>> {
        match self {
            Shape::Rect([u0, v0, u1, v1]) => {
                let c = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
                (0..4)
                    .map(|k| {
                        let (a, b) = (c[k], c[(k + 1) % 4]);
                        let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
                        vec![[[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]]
                    })
                    .collect()
            }
            Shape::Disc(c, r) => {
                let w = std::f64::consts::FRAC_1_SQRT_2;
                let e = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]];
                let arc = |k: usize| {
                    let (a, b) = (e[k], e[(k + 1) % 4]);
                    let p = |x: [f64; 2]| [c[0] + r * x[0], c[1] + r * x[1]];
                    let (pa, pb, pm) = (p(a), p(b), p([a[0] + b[0], a[1] + b[1]]));
                    [[pa[0], pa[1], 1.0], [pm[0], pm[1], w], [pb[0], pb[1], 1.0]]
                };
                vec![vec![arc(0), arc(1)], vec![arc(2), arc(3)]]
            }
        }
    }
}

/// A trim of a coedge whose edge runs between two vertices of its own, of
/// positive length, band 0.
fn trim(coedge: CoedgeId, reversed: bool, spans: &[UvSpan]) -> Trim<'_> {
    Trim {
        coedge,
        reversed,
        spans,
        ends: [VertId(2 * coedge.0), VertId(2 * coedge.0 + 1)],
        zero_length: false,
        band: 0.0,
    }
}

/// A coedge's spans run the other way.
fn backwards(spans: &[UvSpan]) -> Vec<UvSpan> {
    spans.iter().rev().map(|s| [s[2], s[1], s[0]]).collect()
}

/// A test face: its shapes (outer first) and stored trims.
struct TestFace {
    outer: Shape,
    holes: Vec<Shape>,
    flipped: bool,
    /// Per coedge: whether reversed, and the stored spans.
    coedges: Vec<(bool, Vec<UvSpan>)>,
    /// Per coedge, its edge's stored ends: the loops' vertices numbered
    /// in loop order.
    ends: Vec<[VertId; 2]>,
}

impl TestFace {
    /// `outer` with `holes`; loops oriented for `flipped`; each coedge
    /// stored against its edge where `against` says.
    fn new(
        outer: Shape,
        holes: Vec<Shape>,
        flipped: bool,
        against: &mut impl FnMut() -> bool,
    ) -> TestFace {
        let mut coedges = Vec::new();
        let mut ends = Vec::new();
        let mut add = |shape: Shape, ccw: bool| {
            let mut loop_ = shape.ccw();
            if !ccw {
                loop_ = loop_.iter().rev().map(|c| backwards(c)).collect();
            }
            let (base, m) = (ends.len() as u32, loop_.len() as u32);
            for (p, c) in loop_.into_iter().enumerate() {
                let p = p as u32;
                let (from, to) = (VertId(base + p), VertId(base + (p + 1) % m));
                if against() {
                    coedges.push((true, backwards(&c)));
                    ends.push([to, from]);
                } else {
                    coedges.push((false, c));
                    ends.push([from, to]);
                }
            }
        };
        add(outer, !flipped);
        for &h in &holes {
            add(h, flipped);
        }
        TestFace {
            outer,
            holes,
            flipped,
            coedges,
            ends,
        }
    }

    fn trims(&self) -> Vec<Trim<'_>> {
        self.coedges
            .iter()
            .enumerate()
            .zip(&self.ends)
            .map(|((k, (reversed, spans)), ends)| Trim {
                ends: *ends,
                ..trim(CoedgeId(k as u32 * 3 + 1), *reversed, spans)
            })
            .collect()
    }

    /// Whether `p` is inside, `None` within `1e-9` of a boundary.
    fn inside(&self, p: [f64; 2]) -> Option<bool> {
        let o = self.outer.depth(p);
        if o.abs() < 1e-9 {
            return None;
        }
        let mut inside = o > 0.0;
        for h in &self.holes {
            let d = h.depth(p);
            if d.abs() < 1e-9 {
                return None;
            }
            inside &= d < 0.0;
        }
        Some(inside)
    }
}

/// A coordinate in `[lo, hi]`, a multiple of `1/8` one time in three.
fn coord(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    let x = rng.range(lo, hi);
    if rng.next_u64().is_multiple_of(3) {
        let s = (x * 8.0).round() / 8.0;
        if (lo..=hi).contains(&s) {
            return s;
        }
    }
    x
}

/// A random shape inside `[u0, v0, u1, v1]` shrunk by `margin`.
fn shape_in(rng: &mut Rng, b: [f64; 4], margin: f64) -> Option<Shape> {
    let [u0, v0, u1, v1] = [b[0] + margin, b[1] + margin, b[2] - margin, b[3] - margin];
    if u1 - u0 < 0.1 || v1 - v0 < 0.1 {
        return None;
    }
    if rng.next_u64().is_multiple_of(2) {
        let a = [coord(rng, u0, u1), coord(rng, u0, u1)];
        let c = [coord(rng, v0, v1), coord(rng, v0, v1)];
        let r = [
            if a[0] < a[1] { a[0] } else { a[1] },
            if c[0] < c[1] { c[0] } else { c[1] },
            if a[0] < a[1] { a[1] } else { a[0] },
            if c[0] < c[1] { c[1] } else { c[0] },
        ];
        (r[2] - r[0] > 0.05 && r[3] - r[1] > 0.05).then_some(Shape::Rect(r))
    } else {
        let room = ((u1 - u0) * 0.5).min_by(v1 - v0);
        let r = coord(rng, 0.03, room * 0.5 + 0.03);
        let c = [coord(rng, u0 + r, u1 - r), coord(rng, v0 + r, v1 - r)];
        (c[0] - r >= u0 && c[0] + r <= u1 && c[1] - r >= v0 && c[1] + r <= v1)
            .then_some(Shape::Disc(c, r))
    }
}

/// The lesser of two, by compare and select.
trait MinBy {
    fn min_by(self, o: f64) -> f64;
}

impl MinBy for f64 {
    fn min_by(self, o: f64) -> f64 {
        if o < self { o } else { self }
    }
}

fn disjoint(a: [f64; 4], b: [f64; 4], gap: f64) -> bool {
    a[2] + gap < b[0] || b[2] + gap < a[0] || a[3] + gap < b[1] || b[3] + gap < a[1]
}

/// A random face in the tiles: an outer shape, up to two holes.
fn random_face(rng: &mut Rng) -> TestFace {
    let domain = [0.0, 0.0, 3.0, 2.0];
    let outer = loop {
        if let Some(s) = shape_in(rng, domain, 0.0) {
            break s;
        }
    };
    let mut holes: Vec<Shape> = Vec::new();
    for _ in 0..rng.next_u64() % 3 {
        if let Some(h) = shape_in(rng, outer.bounds(), 0.02) {
            let inner = match outer {
                Shape::Rect(_) => true,
                Shape::Disc(c, r) => {
                    let b = h.bounds();
                    [[b[0], b[1]], [b[2], b[1]], [b[0], b[3]], [b[2], b[3]]]
                        .iter()
                        .all(|p| dist(*p, c) < r - 0.02)
                }
            };
            if inner && holes.iter().all(|o| disjoint(o.bounds(), h.bounds(), 0.02)) {
                holes.push(h);
            }
        }
    }
    let flipped = rng.next_u64().is_multiple_of(4);
    let mut r2 = Rng::new(rng.next_u64());
    TestFace::new(outer, holes, flipped, &mut || {
        r2.next_u64().is_multiple_of(2)
    })
}

fn face_uv<'a>(trims: &'a [Trim<'a>], flipped: bool) -> FaceUv<'a> {
    FaceUv::new(Operand::B, FaceId(7), TILES, [1.0, 1.0], trims, flipped).unwrap()
}

/// Refines from the tiles: mixed boxes split (off `q` where given) while
/// shallower than `depth`; the leaves with their classes and the splits
/// taken.
fn refine(
    face: &FaceUv<'_>,
    ps: &[Patch],
    q: Option<&QShadow>,
    depth: u32,
) -> (Vec<(BoxKey, Class)>, Vec<Split>) {
    let mut queue: VecDeque<BoxKey> = face.tile_boxes().collect();
    let (mut leaves, mut splits) = (Vec::new(), Vec::new());
    while let Some(b) = queue.pop_front() {
        let c = classify(face, &b, &first()).expect("classified").class;
        if matches!(c, Class::Mixed(_)) && b.depth() < depth {
            let s = split(face, &b, patch_of(ps, &b), q).expect("split");
            queue.extend(s.halves);
            splits.push(s);
        } else {
            leaves.push((b, c));
        }
        assert!(leaves.len() + queue.len() < 200_000, "refinement runs away");
    }
    (leaves, splits)
}

// ---- The exact oracle: does a stored span meet a closed box? ----

/// `a·t² + b·t + c`.
#[derive(Clone, Debug)]
struct Quad {
    a: Q,
    b: Q,
    c: Q,
}

impl Quad {
    /// From Bernstein coefficients.
    fn bern(c: [Q; 3]) -> Quad {
        let [c0, c1, c2] = c;
        Quad {
            a: &c0 - Q::from(2) * &c1 + &c2,
            b: Q::from(2) * (&c1 - &c0),
            c: c0,
        }
    }

    fn line(b: i64, c: i64) -> Quad {
        Quad {
            a: Q::from(0),
            b: Q::from(b),
            c: Q::from(c),
        }
    }

    fn eval(&self, t: &Q) -> Q {
        (&self.a * t + &self.b) * t + &self.c
    }

    fn is_zero(&self) -> bool {
        self.a == 0 && self.b == 0 && self.c == 0
    }
}

/// A real root of a quadratic: rational, or `(−B ± √D)/(2A)`.
#[derive(Clone, Debug)]
enum Root {
    Rat(Q),
    Surd { a: Q, b: Q, d: Q, plus: bool },
}

fn sign(x: &Q) -> Ordering {
    x.cmp(&Q::from(0))
}

/// The sign of `x + y·√d`, `d ≥ 0`.
fn sign_surd(x: &Q, y: &Q, d: &Q) -> Ordering {
    let (sx, sy) = (sign(x), if *d == 0 { Ordering::Equal } else { sign(y) });
    if sy == Ordering::Equal {
        return sx;
    }
    if sx == Ordering::Equal || sx == sy {
        return sy;
    }
    // Opposite signs: the larger magnitude wins.
    match (x * x).cmp(&(y * y * d)) {
        Ordering::Greater => sx,
        Ordering::Less => sy,
        Ordering::Equal => Ordering::Equal,
    }
}

/// The sign of `h` at the root.
fn sign_at(h: &Quad, r: &Root) -> Ordering {
    match r {
        Root::Rat(t) => sign(&h.eval(t)),
        Root::Surd { a, b, d, plus } => {
            // h mod g with g = a·t² + b·t + c: αt + β, c from d.
            let c = (b * b - d) / (Q::from(4) * a);
            let k = &h.a / a;
            let alpha = &h.b - &k * b;
            let beta = &h.c - &k * &c;
            // α·(−b ± √d)/(2a) + β = (X ± α√d)/(2a), X = 2aβ − αb.
            let x = Q::from(2) * a * &beta - &alpha * b;
            let y = if *plus { alpha } else { -alpha };
            let s = sign_surd(&x, &y, d);
            if sign(a) == Ordering::Less {
                s.reverse()
            } else {
                s
            }
        }
    }
}

/// The real roots of `g` (not identically zero).
fn roots(g: &Quad) -> Vec<Root> {
    if g.a == 0 {
        if g.b == 0 {
            return Vec::new();
        }
        return vec![Root::Rat(-(&g.c / &g.b))];
    }
    let d = &g.b * &g.b - Q::from(4) * &g.a * &g.c;
    match sign(&d) {
        Ordering::Less => Vec::new(),
        Ordering::Equal => vec![Root::Rat(-(&g.b / (Q::from(2) * &g.a)))],
        Ordering::Greater => [true, false]
            .map(|plus| Root::Surd {
                a: g.a.clone(),
                b: g.b.clone(),
                d: d.clone(),
                plus,
            })
            .to_vec(),
    }
}

/// Whether every constraint is `≥ 0` at the root.
fn holds(cons: &[Quad], r: &Root) -> bool {
    cons.iter().all(|h| sign_at(h, r) != Ordering::Less)
}

/// Whether the span meets the closed box `[[u_lo, u_hi], [v_lo, v_hi]]`,
/// exactly on the stored values.
fn meets_exact(s: &UvSpan, bx: &[[Q; 2]; 2]) -> bool {
    // The control points' box apart from the box: the span is in their
    // hull (positive weights), so it misses.
    let lo = |k: usize| s.iter().map(|p| q(p[k])).min().expect("three points");
    let hi = |k: usize| s.iter().map(|p| q(p[k])).max().expect("three points");
    if (0..2).any(|k| hi(k) < bx[k][0] || lo(k) > bx[k][1]) {
        return false;
    }
    let inside = |p: &[f64; 3]| (0..2).all(|k| bx[k][0] <= q(p[k]) && q(p[k]) <= bx[k][1]);
    if inside(&s[0]) || inside(&s[2]) {
        return true;
    }
    let coord = |k: usize, at: &Q, sgn: i64| {
        Quad::bern(std::array::from_fn(|i| {
            Q::from(sgn) * q(s[i][2]) * (q(s[i][k]) - at)
        }))
    };
    let range = [Quad::line(1, 0), Quad::line(-1, 1)];
    for k in 0..2 {
        let o = 1 - k;
        let mut cons = vec![coord(o, &bx[o][0], 1), coord(o, &bx[o][1], -1)];
        cons.extend(range.iter().cloned());
        for end in &bx[k] {
            let g = coord(k, end, 1);
            let candidates: Vec<Root> = if g.is_zero() {
                let mut c = vec![Root::Rat(Q::from(0)), Root::Rat(Q::from(1))];
                for h in &cons[..2] {
                    if !h.is_zero() {
                        c.extend(roots(h));
                    }
                }
                c
            } else {
                roots(&g)
            };
            if candidates.iter().any(|r| holds(&cons, r)) {
                return true;
            }
        }
    }
    false
}

fn exact_box(b: &BoxKey) -> [[Q; 2]; 2] {
    let s = b.sides(&Q::from(0));
    s.map(|[lo, hi]| [lo, hi])
}

fn centre(b: &BoxKey) -> [f64; 2] {
    let [u, v] = b
        .sides(&first())
        .map(|[lo, hi]| (lo.enclose()[0] + hi.enclose()[0]) * 0.5);
    [u, v]
}

/// Checks the leaves of `face`: clear boxes never meet a trim, exactly,
/// and lie on the side their class says; returns the counts of core and
/// outside boxes.
fn check_leaves(tf: &TestFace, leaves: &[(BoxKey, Class)]) -> (usize, usize) {
    let (mut core, mut outside) = (0, 0);
    for (b, c) in leaves {
        let want = match c {
            Class::Core => true,
            Class::Outside => false,
            Class::Mixed(_) => continue,
        };
        if want {
            core += 1;
        } else {
            outside += 1;
        }
        let bx = exact_box(b);
        for (_, spans) in &tf.coedges {
            for s in spans {
                assert!(!meets_exact(s, &bx), "{c:?} box {b:?} meets span {s:?}");
            }
        }
        if let Some(inside) = tf.inside(centre(b)) {
            assert_eq!(inside, want, "{c:?} box {b:?}");
        }
    }
    (core, outside)
}

#[test]
fn clear_boxes_never_meet_the_trims() {
    let ps = patches(None);
    let (mut core, mut outside) = (0, 0);
    for seed in varde_testing::seeds(12, 200) {
        let mut rng = Rng::new(seed);
        let tf = random_face(&mut rng);
        let trims = tf.trims();
        let face = face_uv(&trims, tf.flipped);
        let (leaves, _) = refine(&face, &ps, None, 7);
        let (c, o) = check_leaves(&tf, &leaves);
        core += c;
        outside += o;
    }
    assert!(
        core > 100 && outside > 100,
        "{core} core, {outside} outside"
    );
}

/// Rectangles whose sides lie on tile lines and dyadic lines, a disc
/// tangent to a tile line and a hole touching nothing.
#[test]
fn trims_on_box_sides_leave_the_boxes_mixed() {
    let ps = patches(None);
    let cases = [
        (Shape::Rect([1.0, 0.5, 2.0, 1.0]), vec![]),
        (
            Shape::Rect([0.25, 0.125, 2.75, 1.875]),
            vec![Shape::Rect([1.0, 0.5, 1.5, 1.0])],
        ),
        (Shape::Disc([1.5, 1.0], 0.5), vec![]),
        (
            Shape::Rect([0.0, 0.0, 3.0, 2.0]),
            vec![Shape::Disc([2.0, 1.0], 0.25)],
        ),
    ];
    for (outer, holes) in cases {
        for flipped in [false, true] {
            let tf = TestFace::new(outer, holes.clone(), flipped, &mut || false);
            let trims = tf.trims();
            let face = face_uv(&trims, flipped);
            let (leaves, _) = refine(&face, &ps, None, 6);
            let (core, _) = check_leaves(&tf, &leaves);
            assert!(core > 0, "{outer:?}");
        }
    }
}

/// Every box lies in one tile, and the boxes of each tile cover it
/// exactly once (their areas sum to 1 and they don't overlap).
#[test]
fn tile_lines_are_fixed_sides() {
    let ps = patches(None);
    for seed in varde_testing::seeds(6, 64) {
        let mut rng = Rng::new(seed);
        let tf = random_face(&mut rng);
        let trims = tf.trims();
        let face = face_uv(&trims, tf.flipped);
        let tile = face
            .tile_boxes()
            .nth((rng.next_u64() % 6) as usize)
            .unwrap();
        let qp = point_on(patch_of(&ps, &tile), [rng.unit(), rng.unit()]);
        let qs = QShadow::of(qp).unwrap();
        for query in [None, Some(&qs)] {
            let (leaves, splits) = refine(&face, &ps, query, 6);
            for s in &splits {
                let e = s.halves[0].extent(s.dir);
                assert!(Dyadic::ZERO < s.at && s.at < Dyadic::ONE);
                assert!(e.lo() < s.at && s.at == e.hi());
            }
            let mut area: Vec<Q> = vec![Q::from(0); 6];
            for (b, _) in &leaves {
                for e in [b.extent(Dir::U), b.extent(Dir::V)] {
                    assert!(Dyadic::ZERO <= e.lo() && e.hi() <= Dyadic::ONE);
                }
                let w = |e: Extent| q(dy(e.hi())) - q(dy(e.lo()));
                let [i, j] = b.tile();
                area[(i * 2 + j) as usize] += w(b.extent(Dir::U)) * w(b.extent(Dir::V));
            }
            assert!(area.iter().all(|a| *a == 1), "{area:?}");
            for pair in leaves.windows(2) {
                assert_ne!(pair[0].0, pair[1].0);
            }
            let mut keys: Vec<BoxKey> = leaves.iter().map(|l| l.0).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), leaves.len());
        }
    }
}

/// The patch's point at local `(s, t)`, rounded.
fn point_on(p: &Patch, st: [f64; 2]) -> [f64; 3] {
    let x = p.eval(&Bounded::exact(st[0]), &Bounded::exact(st[1]));
    x.map(|c| {
        let [lo, hi] = c.enclose();
        lo + (hi - lo) * 0.5
    })
}

/// The shadow coordinates of `x`.
fn shadow(x: [f64; 3]) -> [f64; 2] {
    let d = |a: glam::DVec3| a.x * x[0] + a.y * x[1] + a.z * x[2];
    [d(R_HAT), d(NU)]
}

/// A query on a box's middle line: the split moves off it, and the line
/// it takes is clear of the query by samples and at the exact level.
#[test]
fn splits_shift_off_the_query() {
    let mut rng = Rng::new(5);
    let ps = patches(Some(&mut rng));
    let trims: [Trim<'_>; 0] = [];
    let face = face_uv(&trims, false);
    for seed in varde_testing::seeds(16, 256) {
        let mut rng = Rng::new(seed);
        let tile = face
            .tile_boxes()
            .nth((rng.next_u64() % 6) as usize)
            .unwrap();
        // A box of the tile, a few splits down.
        let mut b = tile;
        for _ in 0..rng.next_u64() % 4 {
            let s = split(&face, &b, patch_of(&ps, &b), None).unwrap();
            b = s.halves[(rng.next_u64() % 2) as usize];
        }
        let dir = if b.extent(Dir::V).width() > b.extent(Dir::U).width() {
            Dir::V
        } else {
            Dir::U
        };
        let (e, along) = (b.extent(dir), b.extent(other(dir)));
        let t = dy(along.lo()) + (dy(along.hi()) - dy(along.lo())) * rng.range(0.1, 0.9);
        let st = match dir {
            Dir::U => [dy(e.mid()), t],
            Dir::V => [t, dy(e.mid())],
        };
        let p = patch_of(&ps, &b);
        let qp = point_on(p, st);
        let q = QShadow::of(qp).unwrap();
        let s = split(&face, &b, p, Some(&q)).unwrap();
        assert_eq!(s.dir, dir);
        assert_ne!(s.at, e.mid(), "the middle line passes the query");
        assert_eq!(s.off_q, Some(true));
        // Samples of the line taken: none at the query's shadow.
        let qs = shadow(qp);
        for k in 0..=64 {
            let t = dy(along.lo()) + (dy(along.hi()) - dy(along.lo())) * k as f64 / 64.0;
            let st = match dir {
                Dir::U => [dy(s.at), t],
                Dir::V => [t, dy(s.at)],
            };
            let x = shadow(point_on(p, st));
            assert!((x[0] - qs[0]).abs() + (x[1] - qs[1]).abs() > 1e-12);
        }
        // Without the query, the middle.
        assert_eq!(split(&face, &b, p, None).unwrap().at, e.mid());
    }
}

/// A line certified off the query at the first level is off it exactly.
#[test]
fn off_query_holds_exactly() {
    let ps = patches(None);
    let exact: Vec<HomPatch<Q>> = TILES[0]
        .clone()
        .flat_map(|i| TILES[1].clone().map(move |j| (i, j)))
        .map(|(i, j)| HomPatch::from_affine(2, 2, net(i, j, &|_, _| 1.0), &Q::from(0)).unwrap())
        .collect();
    let trims: [Trim<'_>; 0] = [];
    let face = face_uv(&trims, false);
    for seed in varde_testing::seeds(16, 256) {
        let mut rng = Rng::new(seed);
        let b = face
            .tile_boxes()
            .nth((rng.next_u64() % 6) as usize)
            .unwrap();
        let qp = point_on(patch_of(&ps, &b), [rng.unit(), rng.unit()]);
        let q = QShadow::of(qp).unwrap();
        for k in 1..16u64 {
            let at = Dyadic::new(k, 4).unwrap();
            for dir in [Dir::U, Dir::V] {
                let mut n = 0;
                if iso_off(patch_of(&ps, &b), dir, at, Extent::WHOLE, &q, &mut n) {
                    let mut m = 0;
                    assert!(iso_off(
                        patch_of(&exact, &b),
                        dir,
                        at,
                        Extent::WHOLE,
                        &q,
                        &mut m
                    ));
                }
            }
        }
    }
}

/// The same boxes and classes from every route, trim order and level.
#[test]
fn boxes_and_classes_are_deterministic() {
    let ps = patches(None);
    // One box, two routes.
    let tile = BoxKey::new(Operand::B, FaceId(7), [0, 0], Extent::WHOLE, Extent::WHOLE);
    let d = |n, e| Dyadic::new(n, e).unwrap();
    let a = tile.split_at(Dir::U, d(1, 1)).unwrap()[0]
        .split_at(Dir::U, d(1, 2))
        .unwrap()[1];
    let b = tile.split_at(Dir::U, d(1, 2)).unwrap()[1]
        .split_at(Dir::U, d(1, 1))
        .unwrap()[0];
    assert_eq!(a, b);
    assert_eq!(a.extent(Dir::U), Extent::new(d(1, 2), d(1, 1)).unwrap());
    for seed in varde_testing::seeds(6, 64) {
        let mut rng = Rng::new(seed);
        let tf = random_face(&mut rng);
        let trims = tf.trims();
        let face = face_uv(&trims, tf.flipped);
        let qp = point_on(&ps[0], [rng.unit(), rng.unit()]);
        let q = QShadow::of(qp).unwrap();
        let (leaves, splits) = refine(&face, &ps, Some(&q), 5);
        assert_eq!(
            (leaves.clone(), splits.clone()),
            refine(&face, &ps, Some(&q), 5)
        );
        // Trims in another order.
        let mut shuffled = trims.clone();
        shuffled.reverse();
        shuffled.rotate_left((rng.next_u64() % trims.len() as u64) as usize);
        let other = face_uv(&shuffled, tf.flipped);
        assert_eq!(leaves, refine(&other, &ps, Some(&q), 5).0);
        // Other levels: what the first level settles, they settle alike.
        for (b, c) in leaves.iter().step_by(7) {
            if matches!(c, Class::Mixed(_)) {
                continue;
            }
            let dd = classify(&face, b, &Bounded::exact(Dd::from(0.0)))
                .unwrap()
                .class;
            let exact = classify(&face, b, &Q::from(0)).unwrap().class;
            assert_eq!((&dd, &exact), (c, c), "{b:?}");
        }
    }
}

#[test]
fn refuses_bad_input() {
    let tf = TestFace::new(
        Shape::Rect([0.5, 0.5, 1.5, 1.5]),
        vec![],
        false,
        &mut || false,
    );
    let trims = tf.trims();
    let empty = [0..3, 1..1];
    assert_eq!(
        FaceUv::new(Operand::A, FaceId(0), empty, [1.0, 1.0], &trims, false).err(),
        Some(BoxError::Grid)
    );
    assert_eq!(
        FaceUv::new(Operand::A, FaceId(0), TILES, [1.0, 0.0], &trims, false).err(),
        Some(BoxError::Grid)
    );
    assert_eq!(
        FaceUv::new(
            Operand::A,
            FaceId(0),
            [0..(1 << 52), 0..1],
            [1.0, 1.0],
            &trims,
            false
        )
        .err(),
        Some(BoxError::Grid)
    );
    let bad = [[[0.0, 0.0, 1.0], [f64::NAN, 0.0, 1.0], [1.0, 0.0, 1.0]]];
    let mut with_bad = trims.clone();
    with_bad.push(trim(CoedgeId(99), false, &bad));
    assert_eq!(
        FaceUv::new(Operand::A, FaceId(0), TILES, [1.0, 1.0], &with_bad, false).err(),
        Some(BoxError::Span {
            coedge: CoedgeId(99),
            span: 0
        })
    );
    let face = face_uv(&trims, false);
    let ps = patches(None);
    let foreign = BoxKey::new(Operand::A, FaceId(7), [0, 0], Extent::WHOLE, Extent::WHOLE);
    assert_eq!(classify(&face, &foreign, &first()), Err(BoxError::Foreign));
    let outside = BoxKey::new(Operand::B, FaceId(7), [3, 0], Extent::WHOLE, Extent::WHOLE);
    assert_eq!(split(&face, &outside, &ps[0], None), Err(BoxError::Foreign));
    // A box at the finest depth can't split.
    let d = |n, e| Dyadic::new(n, e).unwrap();
    let fine = Extent::new(d(1, MAX_BOX_DEPTH), d(2, MAX_BOX_DEPTH)).unwrap();
    let tiny = BoxKey::new(Operand::B, FaceId(7), [0, 0], fine, fine);
    assert_eq!(split(&face, &tiny, &ps[0], None), Err(BoxError::Depth));
    // Two outer loops the same way: winding 2 round their common part.
    let twice = TestFace::new(
        Shape::Rect([0.5, 0.5, 1.5, 1.5]),
        vec![],
        false,
        &mut || false,
    );
    let mut both = trims.clone();
    both.extend(twice.trims().into_iter().map(|mut t| {
        t.coedge = CoedgeId(t.coedge.0 + 1000);
        t
    }));
    let face = face_uv(&both, false);
    let mid = Extent::new(d(5, 3), d(7, 3)).unwrap();
    let b = BoxKey::new(Operand::B, FaceId(7), [0, 0], mid, mid);
    assert_eq!(classify(&face, &b, &first()), Err(BoxError::Winding(2)));
}

/// A box at the finest depth whose middle (or a shifted position) lies on
/// the finest grid still splits there: positions are taken in lowest terms.
#[test]
fn finest_boxes_split_where_the_grid_allows() {
    let trims: [Trim<'_>; 0] = [];
    let face = face_uv(&trims, false);
    let ps = patches(None);
    let d = |n, e| Dyadic::new(n, e).unwrap();
    let m = MAX_BOX_DEPTH;
    // Width two units of the finest grid: the middle is on it.
    let two = Extent::new(d(1, m), d(3, m)).unwrap();
    let b = BoxKey::new(Operand::B, FaceId(7), [0, 0], two, two);
    let s = split(&face, &b, &ps[0], None).unwrap();
    assert_eq!((s.dir, s.at), (Dir::U, d(1, m - 1)));
    // Width sixteen units: every position is on it.
    let wide = Extent::new(d(1, m), d(17, m)).unwrap();
    let b = BoxKey::new(Operand::B, FaceId(7), [0, 0], wide, wide);
    let s = split(&face, &b, &ps[0], None).unwrap();
    assert_eq!((s.dir, s.at), (Dir::U, d(9, m)));
}

// ---- Polygons against an exact winding oracle ----

/// A straight span from `a` to `b`: the middle control point at `λ` of
/// the way, weights from `w` (any positive weights keep it the segment).
fn segment(a: [f64; 2], b: [f64; 2], lambda: f64, w: [f64; 3]) -> UvSpan {
    let m = [a[0] + (b[0] - a[0]) * lambda, a[1] + (b[1] - a[1]) * lambda];
    [[a[0], a[1], w[0]], [m[0], m[1], w[1]], [b[0], b[1], w[2]]]
}

/// The exact winding of the closed polylines `loops` (each a vertex list,
/// closed back to its first) about `p`, by a ray along `+u` just below
/// `p` (a vertex at `p`'s height counts above); `p` off every segment.
fn polygon_winding(loops: &[Vec<[f64; 2]>], p: &[Q; 2]) -> i64 {
    let mut w = 0;
    for l in loops {
        for k in 0..l.len() {
            let (a, b) = (l[k], l[(k + 1) % l.len()]);
            let (a, b) = ([q(a[0]), q(a[1])], [q(b[0]), q(b[1])]);
            let (sa, sb) = (a[1] >= p[1], b[1] >= p[1]);
            if sa == sb {
                continue;
            }
            let u = &a[0] + (&p[1] - &a[1]) * (&b[0] - &a[0]) / (&b[1] - &a[1]);
            assert_ne!(u, p[0], "the point is on a segment");
            if u > p[0] {
                w += if sb { 1 } else { -1 };
            }
        }
    }
    w
}

/// A random polyline loop of `n` vertices on the `1/32` grid of the
/// domain, often along tile lines and through tile corners.
fn grid_loop(rng: &mut Rng, n: usize) -> Vec<[f64; 2]> {
    let g = |rng: &mut Rng, hi: u64| (rng.next_u64() % (hi * 32 + 1)) as f64 / 32.0;
    let mut l: Vec<[f64; 2]> = Vec::new();
    while l.len() < n {
        let p = match rng.next_u64() % 4 {
            // On a tile line.
            0 => [(rng.next_u64() % 4) as f64, g(rng, 2)],
            1 => [g(rng, 3), (rng.next_u64() % 3) as f64],
            _ => [g(rng, 3), g(rng, 2)],
        };
        if l.last() != Some(&p) {
            l.push(p);
        }
    }
    if l[0] == l[n - 1] {
        l.pop();
    }
    l
}

/// The coedges of `loops`: each segment one or more spans (a split at a
/// grid point), a zero-length span now and then, stored either way.
fn polygon_trims(rng: &mut Rng, loops: &[Vec<[f64; 2]>]) -> Vec<(bool, Vec<UvSpan>)> {
    let weights = [0.25, 0.5, 1.0, 2.0, 3.0];
    let wt = |rng: &mut Rng| weights[(rng.next_u64() % 5) as usize];
    let mut out = Vec::new();
    for l in loops {
        let mut spans = Vec::new();
        for k in 0..l.len() {
            let (a, b) = (l[k], l[(k + 1) % l.len()]);
            let lambda = [0.25, 0.5, 0.75][(rng.next_u64() % 3) as usize];
            if rng.next_u64().is_multiple_of(3) {
                // Two spans joining at the middle, exactly.
                let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
                spans.push(segment(a, m, lambda, [wt(rng), wt(rng), wt(rng)]));
                spans.push(segment(m, b, lambda, [wt(rng), wt(rng), wt(rng)]));
            } else {
                spans.push(segment(a, b, lambda, [wt(rng), wt(rng), wt(rng)]));
            }
            if rng.next_u64().is_multiple_of(7) {
                spans.push(segment(b, b, 0.5, [wt(rng), wt(rng), wt(rng)]));
            }
        }
        // Cut into coedges of one to three spans.
        let mut rest = &spans[..];
        while !rest.is_empty() {
            let k = 1 + (rng.next_u64() % 3) as usize;
            let (c, r) = rest.split_at(k.min(rest.len()));
            rest = r;
            if rng.next_u64().is_multiple_of(2) {
                out.push((true, backwards(c)));
            } else {
                out.push((false, c.to_vec()));
            }
        }
    }
    out
}

/// Random polylines, self-crossing or not, with spans of any weights,
/// trims along tile lines, through box corners and centres' heights:
/// every box the refinement leaves clear is clear exactly, and its class
/// (or the refusal) is the exact winding about its centre.
#[test]
fn classes_match_the_exact_winding_of_polygons() {
    let ps = patches(None);
    let (mut core, mut outside, mut refused) = (0, 0, 0);
    for seed in varde_testing::seeds(6, 400) {
        let mut rng = Rng::new(seed);
        let n_loops = 1 + (rng.next_u64() % 3) as usize;
        let loops: Vec<Vec<[f64; 2]>> = (0..n_loops)
            .map(|_| {
                let n = 3 + (rng.next_u64() % 12) as usize;
                grid_loop(&mut rng, n)
            })
            .filter(|l| l.len() >= 2)
            .collect();
        let coedges = polygon_trims(&mut rng, &loops);
        let trims: Vec<Trim<'_>> = coedges
            .iter()
            .enumerate()
            .map(|(k, (reversed, spans))| trim(CoedgeId(k as u32), *reversed, spans))
            .collect();
        let flipped = rng.next_u64().is_multiple_of(3);
        let face = face_uv(&trims, flipped);
        let sign = if flipped { -1 } else { 1 };
        let tile = face
            .tile_boxes()
            .nth((rng.next_u64() % 6) as usize)
            .unwrap();
        let qs = QShadow::of(point_on(patch_of(&ps, &tile), [rng.unit(), rng.unit()])).unwrap();
        let mut queue: VecDeque<BoxKey> = face.tile_boxes().collect();
        while let Some(b) = queue.pop_front() {
            let got = classify(&face, &b, &first()).map(|c| c.class);
            if let Ok(Class::Mixed(_)) = got {
                if b.depth() < 6 {
                    let s = split(&face, &b, patch_of(&ps, &b), Some(&qs)).unwrap();
                    queue.extend(s.halves);
                }
                continue;
            }
            let bx = exact_box(&b);
            for (_, spans) in &coedges {
                for s in spans {
                    assert!(!meets_exact(s, &bx), "{got:?} box {b:?} meets {s:?}");
                }
            }
            let w = polygon_winding(&loops, &b.centre_exact());
            let want = if w == 0 {
                outside += 1;
                Ok(Class::Outside)
            } else if w == sign {
                core += 1;
                Ok(Class::Core)
            } else {
                refused += 1;
                Err(BoxError::Winding(w))
            };
            assert_eq!(got, want, "box {b:?} seed {seed}");
            if rng.next_u64().is_multiple_of(50) {
                let exact = classify(&face, &b, &Q::from(0)).map(|c| c.class);
                assert_eq!(exact, want, "exact level, box {b:?}");
            }
        }
    }
    assert!(
        core > 50 && outside > 50 && refused > 5,
        "{core} {outside} {refused}"
    );
}

/// A hole far smaller than a tile, across a tile line, and a sliver of a
/// face along one, refined only where they are: the boxes deep down
/// classify by the exact winding, and some lie inside the hole.
#[test]
fn tiny_holes_resolve_deep_down() {
    let ps = patches(None);
    let e = 1.0 / (1u64 << 31) as f64;
    for (hole, flipped) in [
        (
            vec![
                [1.0 - e, 0.5],
                [1.0 + e, 0.5],
                [1.0 + e, 0.5 + e],
                [1.0 - e, 0.5 + e],
            ],
            false,
        ),
        (
            vec![[2.0, 1.0], [2.0 + e, 1.0 - e], [2.0 + 2.0 * e, 1.0]],
            true,
        ),
        (
            vec![
                [0.75, 1.0 - e],
                [0.75 + 3.0 * e, 1.0 - e],
                [0.75 + e, 1.0 + 2.0 * e],
            ],
            false,
        ),
    ] {
        let outer = vec![[0.25, 0.25], [2.75, 0.25], [2.75, 1.75], [0.25, 1.75]];
        // Outer counter-clockwise and the hole clockwise for an unflipped
        // face, the other way for a flipped one.
        let ccw = |mut l: Vec<[f64; 2]>, ccw: bool| {
            let area: f64 = (0..l.len())
                .map(|k| {
                    // About the first vertex: exact for these small loops.
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    let (a, b) = (
                        [a[0] - l[0][0], a[1] - l[0][1]],
                        [b[0] - l[0][0], b[1] - l[0][1]],
                    );
                    a[0] * b[1] - a[1] * b[0]
                })
                .sum();
            if (area > 0.0) != ccw {
                l.reverse();
            }
            l
        };
        let loops = vec![ccw(outer, !flipped), ccw(hole, flipped)];
        let mut rng = Rng::new(3);
        let coedges = polygon_trims(&mut rng, &loops);
        let trims: Vec<Trim<'_>> = coedges
            .iter()
            .enumerate()
            .map(|(k, (reversed, spans))| trim(CoedgeId(k as u32), *reversed, spans))
            .collect();
        // The hole's coedges: those whose spans come near it.
        let near = |s: &UvSpan| {
            s.iter()
                .all(|p| (p[0] - 1.5).abs() < 1.4 && (p[1] - 1.0).abs() < 0.7)
        };
        let hole_coedges: Vec<CoedgeId> = trims
            .iter()
            .filter(|t| t.spans.iter().all(near))
            .map(|t| t.coedge)
            .collect();
        let face = face_uv(&trims, flipped);
        let sign = if flipped { -1 } else { 1 };
        let mut queue: VecDeque<BoxKey> = face.tile_boxes().collect();
        let (mut in_hole, mut deepest) = (0, 0);
        while let Some(b) = queue.pop_front() {
            let class = match classify(&face, &b, &first()) {
                Ok(c) => c.class,
                Err(e) => panic!(
                    "{e:?} box {b:?} oracle {} loops {loops:?}",
                    polygon_winding(&loops, &b.centre_exact())
                ),
            };
            match class {
                Class::Mixed(cs) => {
                    if b.depth() < 35 && cs.iter().any(|c| hole_coedges.contains(c)) {
                        let s = split(&face, &b, patch_of(&ps, &b), None).unwrap();
                        queue.extend(s.halves);
                    }
                }
                c => {
                    let bx = exact_box(&b);
                    for (_, spans) in &coedges {
                        for s in spans {
                            assert!(!meets_exact(s, &bx), "{c:?} box {b:?} meets {s:?}");
                        }
                    }
                    let w = polygon_winding(&loops, &b.centre_exact());
                    assert_eq!(c, if w == 0 { Class::Outside } else { Class::Core });
                    assert!(w == 0 || w == sign);
                    if w == 0 && b.depth() > 25 {
                        let p = b.centre_exact();
                        if polygon_winding(&loops[1..], &p) != 0 {
                            in_hole += 1;
                        }
                    }
                    deepest = deepest.max(b.depth());
                }
            }
            assert!(queue.len() < 100_000, "refinement runs away");
        }
        assert!(in_hole > 0 && deepest > 30, "{in_hole} {deepest}");
    }
}

/// The exact winding about `p` of closed chains of rational quadratic
/// spans (`loops`, each closed, spans in order): the winding of the
/// chords plus, for each span whose lens (between the arc and its chord)
/// holds `p`, the lens's orientation. `None` where `p` is on a chord.
fn arc_winding(loops: &[Vec<UvSpan>], p: &[Q; 2]) -> Option<i64> {
    let mut w = 0;
    for l in loops {
        let chords: Vec<[f64; 2]> = l.iter().map(|s| [s[0][0], s[0][1]]).collect();
        let on_chord = (0..chords.len()).any(|k| {
            let (a, b) = (chords[k], chords[(k + 1) % chords.len()]);
            let (a, b) = ([q(a[0]), q(a[1])], [q(b[0]), q(b[1])]);
            let cross = (&b[0] - &a[0]) * (&p[1] - &a[1]) - (&b[1] - &a[1]) * (&p[0] - &a[0]);
            let within = |k: usize| {
                let (lo, hi) = if a[k] <= b[k] {
                    (&a[k], &b[k])
                } else {
                    (&b[k], &a[k])
                };
                *lo <= p[k] && p[k] <= *hi
            };
            cross == 0 && within(0) && within(1)
        });
        if on_chord {
            return None;
        }
        w += polygon_winding(std::slice::from_ref(&chords), p);
        for s in l {
            let pt = |k: usize| [q(s[k][0]), q(s[k][1])];
            let (a, m, b) = (pt(0), pt(1), pt(2));
            let area = |x: &[Q; 2], y: &[Q; 2], z: &[Q; 2]| {
                (&y[0] - &x[0]) * (&z[1] - &x[1]) - (&y[1] - &x[1]) * (&z[0] - &x[0])
            };
            let d = area(&a, &m, &b);
            if d == 0 {
                continue;
            }
            // Barycentric coordinates of `p` in `(a, m, b)`.
            let t0 = area(p, &m, &b) / &d;
            let t1 = area(&a, p, &b) / &d;
            let t2 = area(&a, &m, p) / &d;
            let (w0, w1, w2) = (q(s[0][2]), q(s[1][2]), q(s[2][2]));
            let zero = Q::from(0);
            let lens = t0 > zero
                && t1 > zero
                && t2 > zero
                && &t1 * &t1 * &w0 * &w2 < Q::from(4) * &w1 * &w1 * &t0 * &t2;
            if lens {
                w += if d > zero { 1 } else { -1 };
            }
        }
    }
    Some(w)
}

/// Loops of bulging arcs of random weights between grid points, crossing
/// each other and tile lines: every box the refinement leaves clear is
/// clear exactly, and its class (or the refusal) is the exact winding.
#[test]
fn classes_match_the_exact_winding_of_arcs() {
    let ps = patches(None);
    let weights = [0.125, 0.5, 0.75, 1.0, 1.5, 4.0];
    let (mut core, mut outside, mut refused) = (0, 0, 0);
    for seed in varde_testing::seeds(10, 200) {
        let mut rng = Rng::new(seed);
        let n_loops = 1 + (rng.next_u64() % 2) as usize;
        let loops: Vec<Vec<UvSpan>> = (0..n_loops)
            .map(|_| {
                let n = 2 + (rng.next_u64() % 6) as usize;
                let l = grid_loop(&mut rng, n);
                (0..l.len())
                    .map(|k| {
                        let (a, b) = (l[k], l[(k + 1) % l.len()]);
                        let off = ((rng.next_u64() % 17) as f64 - 8.0) / 16.0;
                        let m = [
                            (a[0] + b[0]) * 0.5 - (b[1] - a[1]) * off,
                            (a[1] + b[1]) * 0.5 + (b[0] - a[0]) * off,
                        ];
                        let mut w = || weights[(rng.next_u64() % 6) as usize];
                        [[a[0], a[1], w()], [m[0], m[1], w()], [b[0], b[1], w()]]
                    })
                    .collect()
            })
            .filter(|l: &Vec<UvSpan>| l.len() >= 2)
            .collect();
        let mut coedges: Vec<(bool, Vec<UvSpan>)> = Vec::new();
        for l in &loops {
            for s in l {
                if rng.next_u64().is_multiple_of(2) {
                    coedges.push((true, backwards(std::slice::from_ref(s))));
                } else {
                    coedges.push((false, vec![*s]));
                }
            }
        }
        let trims: Vec<Trim<'_>> = coedges
            .iter()
            .enumerate()
            .map(|(k, (reversed, spans))| trim(CoedgeId(k as u32), *reversed, spans))
            .collect();
        let flipped = rng.next_u64().is_multiple_of(3);
        let face = face_uv(&trims, flipped);
        let sign = if flipped { -1 } else { 1 };
        let mut queue: VecDeque<BoxKey> = face.tile_boxes().collect();
        while let Some(b) = queue.pop_front() {
            let got = classify(&face, &b, &first()).map(|c| c.class);
            if let Ok(Class::Mixed(_)) = got {
                if b.depth() < 6 {
                    let s = split(&face, &b, patch_of(&ps, &b), None).unwrap();
                    queue.extend(s.halves);
                }
                continue;
            }
            let bx = exact_box(&b);
            for (_, spans) in &coedges {
                for s in spans {
                    assert!(!meets_exact(s, &bx), "{got:?} box {b:?} meets {s:?}");
                }
            }
            let Some(w) = arc_winding(&loops, &b.centre_exact()) else {
                continue;
            };
            let want = if w == 0 {
                outside += 1;
                Ok(Class::Outside)
            } else if w == sign {
                core += 1;
                Ok(Class::Core)
            } else {
                refused += 1;
                Err(BoxError::Winding(w))
            };
            assert_eq!(got, want, "box {b:?} seed {seed} loops {loops:?}");
        }
    }
    assert!(core > 20 && outside > 20, "{core} {outside} {refused}");
}

/// A box at any depth beside a long trim, as near it as its own size,
/// classifies: the span's halvings reach as deep as the boxes go.
#[test]
fn deep_boxes_beside_long_trims_classify() {
    // The triangle above the tile's diagonal, counter-clockwise.
    let l = [[0.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let spans: Vec<UvSpan> = (0..3)
        .map(|k| segment(l[k], l[(k + 1) % 3], 0.5, [1.0, 1.0, 1.0]))
        .collect();
    let trims: Vec<Trim<'_>> = spans
        .chunks(1)
        .enumerate()
        .map(|(k, s)| trim(CoedgeId(k as u32), false, s))
        .collect();
    let face = face_uv(&trims, false);
    for depth in [20, 36, 44, MAX_BOX_DEPTH] {
        // `[x, x + h] × [x + 2h, x + 3h]`, `h = 2^-depth`, `x` near ⅓:
        // above the diagonal, a box's width from it.
        let k = ((1u64 << depth) / 3) | 1;
        let d = |n: u64| Dyadic::new(n, depth).unwrap();
        let u = Extent::new(d(k), d(k + 1)).unwrap();
        let v = Extent::new(d(k + 2), d(k + 3)).unwrap();
        let b = BoxKey::new(Operand::B, FaceId(7), [0, 0], u, v);
        // The first level's rounding in the halvings outgrows the finest
        // boxes' width: there the next level decides.
        let mut got = vec![
            classify(&face, &b, &Bounded::exact(Dd::from(0.0))),
            classify(&face, &b, &Q::from(0)),
        ];
        if depth <= 44 {
            got.push(classify(&face, &b, &first()));
        }
        for got in got {
            assert_eq!(got.map(|c| c.class), Ok(Class::Core), "depth {depth}");
        }
    }
}

/// Random faces on the curved surface refined by role, each box from its
/// parent's spans: thin boxes split, strip and corner boxes until their
/// hull is within the band. No box is left thin; a strip box is clear of
/// every other coedge's spans, exactly; a corner box meets only its
/// corner's coedges, near the corner's point; a box's role and met spans
/// from its parent's spans are those from all spans.
#[test]
fn strips_and_corners_of_random_faces() {
    let band = 1.0 / 64.0;
    let (mut strips, mut corner_boxes) = (0, 0);
    for seed in varde_testing::seeds(6, 48) {
        let mut rng = Rng::new(seed);
        let tf = random_face(&mut rng);
        let trims: Vec<Trim<'_>> = tf.trims().into_iter().map(|t| Trim { band, ..t }).collect();
        let face = face_uv(&trims, tf.flipped);
        // Weights drawn over the whole grid: one surface across tile lines.
        let ps = patches(Some(&mut rng));
        let corners = Corners::new(&face, &ps).expect("corners");
        let mut queue: VecDeque<(BoxKey, Option<SmallVec<[SpanRef; 8]>>)> =
            face.tile_boxes().map(|b| (b, None)).collect();
        let mut boxes = 0;
        while let Some((b, among)) = queue.pop_front() {
            boxes += 1;
            assert!(boxes < 60_000, "refinement runs away");
            let r = role(&face, &corners, &b, &first(), among.as_deref()).expect("role");
            if among.is_some() {
                let all = role(&face, &corners, &b, &first(), None).expect("role");
                assert_eq!((&all.role, &all.met), (&r.role, &r.met), "box {b:?}");
            }
            let patch = patch_of(&ps, &b);
            let more = match &r.role {
                Role::Thin(c) => {
                    assert!(b.depth() < 16, "box {b:?} left thin, meeting {c:?}");
                    true
                }
                Role::Strip(_) | Role::Corner { .. } => {
                    !within_band(&box_hull(patch, &b).expect("positive weights"), band)
                }
                Role::Core | Role::Outside => false,
            };
            if more {
                let s = split(&face, &b, patch, None).expect("split");
                for h in s.halves {
                    queue.push_back((h, Some(r.met.clone())));
                }
                continue;
            }
            let bx = exact_box(&b);
            match &r.role {
                Role::Strip(set) => {
                    strips += 1;
                    for t in face.trims().iter().filter(|t| !set.contains(&t.coedge)) {
                        for s in t.spans {
                            assert!(!meets_exact(s, &bx), "strip {b:?} of {set:?} meets {t:?}");
                        }
                    }
                }
                Role::Corner { vertex, met } => {
                    corner_boxes += 1;
                    let corner = corners.corner(*vertex).expect("a corner");
                    assert!(met.iter().all(|c| corner.curves.contains(c)), "box {b:?}");
                    // The corner's point: the end of a curve there.
                    let t = trims.iter().find(|t| t.coedge == corner.curves[0]).unwrap();
                    let p = if corner.vertices.contains(&t.ends[0]) {
                        t.spans[0][0]
                    } else {
                        t.spans[t.spans.len() - 1][2]
                    };
                    let c = centre(&b);
                    let r = rect_size(&b);
                    assert!(
                        dist([p[0], p[1]], c) <= 4.0 * band + r,
                        "corner box {b:?} far from {p:?}"
                    );
                }
                _ => {}
            }
        }
    }
    assert!(strips > 500 && corner_boxes > 20, "{strips} {corner_boxes}");
}

/// The box's UV diagonal.
fn rect_size(b: &BoxKey) -> f64 {
    let s = b
        .sides(&first())
        .map(|[lo, hi]| hi.enclose()[0] - lo.enclose()[0]);
    (s[0] * s[0] + s[1] * s[1]).sqrt()
}

/// The surface's point at global `uv`, by its tile's patch.
fn surface_at(ps: &[Patch], uv: [f64; 2]) -> [f64; 3] {
    let tile = [
        (uv[0].floor() as i64).clamp(TILES[0].start, TILES[0].end - 1),
        (uv[1].floor() as i64).clamp(TILES[1].start, TILES[1].end - 1),
    ];
    let nv = (TILES[1].end - TILES[1].start) as usize;
    let p = &ps[(tile[0] - TILES[0].start) as usize * nv + (tile[1] - TILES[1].start) as usize];
    point_on(p, [uv[0] - tile[0] as f64, uv[1] - tile[1] as f64])
}

/// Tubes on the weighted curved surface, round curved spans crossing tile
/// lines: a box certified in the tube has every sampled point within the
/// band of the sampled curve (up to the samples' spacing).
#[test]
fn tubes_on_the_curved_surface_hold_by_samples() {
    let mut certified = 0;
    for seed in varde_testing::seeds(8, 32) {
        let mut rng = Rng::new(seed);
        let ps = patches(Some(&mut rng));
        let pt = |rng: &mut Rng| [rng.range(0.1, 2.9), rng.range(0.1, 1.9)];
        let (a, b, c) = (pt(&mut rng), pt(&mut rng), pt(&mut rng));
        let span: UvSpan = [
            [a[0], a[1], 1.0],
            [c[0], c[1], rng.range(0.3, 3.0)],
            [b[0], b[1], 1.0],
        ];
        let spans = [span];
        let band = rng.log_range(0.003, 0.1);
        let trims = [Trim {
            band,
            ..trim(CoedgeId(0), false, &spans)
        }];
        let face = face_uv(&trims, false);
        let at = |t: f64| {
            let bs = [(1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t, t * t];
            let w: f64 = (0..3).map(|i| bs[i] * span[i][2]).sum();
            let p: [f64; 2] = std::array::from_fn(|k| {
                (0..3).map(|i| bs[i] * span[i][2] * span[i][k]).sum::<f64>() / w
            });
            p
        };
        let curve: Vec<[f64; 3]> = (0..=10_000)
            .map(|i| surface_at(&ps, at(i as f64 / 10_000.0)))
            .collect();
        let spacing = curve
            .windows(2)
            .map(|w| {
                let d = [w[0][0] - w[1][0], w[0][1] - w[1][1], w[0][2] - w[1][2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
            })
            .fold(0.0, |m: f64, x| if x > m { x } else { m });
        for _ in 0..200 {
            let p = at(rng.unit());
            let p = [
                p[0] + rng.range(-1.5, 1.5) * band,
                p[1] + rng.range(-1.5, 1.5) * band,
            ];
            if !(0.0..3.0).contains(&p[0]) || !(0.0..2.0).contains(&p[1]) {
                continue;
            }
            let size = band * rng.log_range(1.0 / 32.0, 1.0);
            let mut d = 1;
            while 1.0 / (1u64 << d) as f64 > size {
                d += 1;
            }
            let tile = [p[0].floor() as i64, p[1].floor() as i64];
            let ext = |x: f64| {
                let k = (x * (1u64 << d) as f64).floor() as u64;
                Extent::new(Dyadic::new(k, d).unwrap(), Dyadic::new(k + 1, d).unwrap()).unwrap()
            };
            let bx = BoxKey::new(
                Operand::B,
                FaceId(7),
                tile,
                ext(p[0] - tile[0] as f64),
                ext(p[1] - tile[1] as f64),
            );
            if !in_tube(&face, &ps, &bx, CoedgeId(0)).unwrap().inside {
                continue;
            }
            certified += 1;
            let patch = patch_of(&ps, &bx);
            let [eu, ev] = [bx.extent(Dir::U), bx.extent(Dir::V)];
            for i in 0..=2 {
                for j in 0..=2 {
                    let st = [
                        dy(eu.lo()) + (dy(eu.hi()) - dy(eu.lo())) * i as f64 / 2.0,
                        dy(ev.lo()) + (dy(ev.hi()) - dy(ev.lo())) * j as f64 / 2.0,
                    ];
                    let x = point_on(patch, st);
                    let m = curve
                        .iter()
                        .map(|y| {
                            let d = [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
                            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                        })
                        .fold(f64::INFINITY, |m, x| if x < m { x } else { m });
                    assert!(m <= band + spacing, "box {bx:?}: {m} > {band}");
                }
            }
        }
    }
    assert!(certified > 200, "{certified}");
}
