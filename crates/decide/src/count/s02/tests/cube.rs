//! A vertex of `A` on a corner of the cube `B = [0, 1]³`,
//! `A` a box with that corner rotated about it. Each cube face at the
//! corner counts its pieces' degrees to its ray records' winding (an
//! integer, the records at the corner read once), its hit at the corner
//! (the point exactly at the vertex: the germ rule) reads the one
//! point–vertex height `VV(v, w)`, decided per face by the face's own
//! plane at the tie, and the counts over the cube are the winding of the
//! moved point: 1 where `A`'s vertex direction points into the cube,
//! else 0.

use varde_testkit::rng::Rng;

use super::*;
use crate::num::decide::Band;
use crate::perturb::{Moving, Rate, Seg};
use crate::records::feet::{EdgeCurve, FeetGeometry};
use crate::records::height::model::{FaceAt, HeightModel, ModelHeights};
use crate::records::keys::{FootId, HKey, RayKey, VvKey};
use crate::records::shadow::ShadowGeometry;

const BAND: f64 = 1.0 / 4096.0;

/// The patch's map: local `s ∈ [−0.3, 1.4]` over the tile, so the face's
/// square `[0, 1]²` is trimmed inside it at non-dyadic parameters.
const LO: f64 = -0.3;
const SPAN: f64 = 1.7;

/// The local size of a tile where the square's sides lie on tile lines
/// ([`Tiling::Lines`]).
const LINES: f64 = 1.25;

/// The square's sides from its corner at the origin in UV where they
/// lie across the tiles ([`Tiling::Skew`]): `(u, v) − (1, 1) = N·(s, t)`.
const SKEW: [[f64; 2]; 2] = [[0.7, 0.2], [0.2, 0.7]];

/// The cube's vertex at corner bits `(x, y, z)`: id `1 + x + 2y + 4z`.
fn vid(c: [u8; 3]) -> VertId {
    VertId(1 + u32::from(c[0]) + 2 * u32::from(c[1]) + 4 * u32::from(c[2]))
}

fn vpos(v: VertId) -> [f64; 3] {
    let k = v.0 - 1;
    [k & 1, (k >> 1) & 1, (k >> 2) & 1].map(f64::from)
}

/// The cube's faces: origin corner, `a`, `b` with `a × b` outward.
fn faces() -> Vec<([u8; 3], [u8; 3], [u8; 3])> {
    let (x, y, z) = ([1, 0, 0], [0, 1, 0], [0, 0, 1]);
    vec![
        ([0, 0, 0], y, x),
        ([0, 0, 1], x, y),
        ([0, 0, 0], z, y),
        ([1, 0, 0], y, z),
        ([0, 0, 0], x, z),
        ([0, 1, 0], z, x),
    ]
}

struct Cube {
    faces: Vec<FaceData>,
    order: Order,
    /// Per edge, its stored ends.
    edges: BTreeMap<EdgeId, [VertId; 2]>,
    /// `A`'s vertex direction at the corner.
    d: DVec3,
    /// `A`'s vertex.
    at: DVec3,
}

/// How a cube face's square sits in its tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tiling {
    /// One tile, the square inside it at non-dyadic parameters.
    One,
    /// Four tiles, the square's corner at the origin on their common
    /// corner `(1, 1)` and its sides there along the tile lines.
    Lines,
    /// Four tiles, the square's corner at the origin on their common
    /// corner `(1, 1)`, its sides across the tiles: the boxes round the
    /// corner lie in four tiles, only one of them inside the face.
    Skew,
}

impl Tiling {
    fn tiles(self) -> std::ops::Range<i64> {
        match self {
            Tiling::One => 0..1,
            Tiling::Lines | Tiling::Skew => 0..2,
        }
    }

    /// The square's local parameters at UV `w`.
    fn st(self, w: [f64; 2]) -> [f64; 2] {
        match self {
            Tiling::One => w.map(|x| LO + SPAN * x),
            Tiling::Lines => w.map(|x| LINES * (x - 1.0)),
            Tiling::Skew => {
                let [[a, b], [c, d]] = SKEW;
                let (x, y) = (w[0] - 1.0, w[1] - 1.0);
                let det = a * d - b * c;
                [(d * x - b * y) / det, (a * y - c * x) / det]
            }
        }
    }

    /// The UV of the square's local parameters `st`.
    fn uv(self, st: [f64; 2]) -> [f64; 2] {
        match self {
            Tiling::One => st.map(|s| (s - LO) / SPAN),
            Tiling::Lines => st.map(|s| 1.0 + s / LINES),
            Tiling::Skew => {
                let [[a, b], [c, d]] = SKEW;
                [1.0 + a * st[0] + b * st[1], 1.0 + c * st[0] + d * st[1]]
            }
        }
    }
}

impl Cube {
    fn new(d: DVec3) -> Cube {
        Cube::tiled(d, Tiling::One)
    }

    fn tiled(d: DVec3, tiling: Tiling) -> Cube {
        let add = |p: [u8; 3], q: [u8; 3]| [0, 1, 2].map(|k| p[k] + q[k]);
        let mut edges: BTreeMap<(VertId, VertId), EdgeId> = BTreeMap::new();
        let mut out = Vec::new();
        for (n, (o, a, b)) in faces().into_iter().enumerate() {
            let corners = [o, add(o, a), add(add(o, a), b), add(o, b)];
            let st = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            let [o3, a3, b3] = [o, a, b].map(|c| DVec3::from_array(c.map(f64::from)));
            let patches: Vec<Patch> = tiling
                .tiles()
                .flat_map(|i| tiling.tiles().map(move |j| [i, j]))
                .map(|[i, j]| {
                    let mut net = Vec::new();
                    for a in 0..3 {
                        for b in 0..3 {
                            let w = [i as f64 + a as f64 / 2.0, j as f64 + b as f64 / 2.0];
                            let [s, t] = tiling.st(w);
                            let p = o3 + a3 * s + b3 * t;
                            net.push([p.x, p.y, p.z, 1.0]);
                        }
                    }
                    Patch::from_affine(2, 2, net, &first()).unwrap()
                })
                .collect();
            let mut trims = Vec::new();
            for k in 0..4 {
                let (p, q) = (vid(corners[k]), vid(corners[(k + 1) % 4]));
                let (lo, hi) = if p < q { (p, q) } else { (q, p) };
                let next = EdgeId(edges.len() as u32 + 1);
                let edge = *edges.entry((lo, hi)).or_insert(next);
                let (sp, sq) = (st[k], st[(k + 1) % 4]);
                let (s0, s1) = if p == lo { (sp, sq) } else { (sq, sp) };
                trims.push(TrimData {
                    coedge: CoedgeId(10 * (n as u32 + 1) + k as u32),
                    edge,
                    reversed: p != lo,
                    spans: vec![pline(tiling.uv(s0), tiling.uv(s1))],
                    ends: [lo, hi],
                    band: BAND,
                    curve: vec![line(vpos(lo), vpos(hi))],
                    maps: whole(1),
                });
            }
            out.push(FaceData {
                operand: Operand::B,
                id: FaceId(n as u32 + 1),
                tiles: [tiling.tiles(), tiling.tiles()],
                nets: Some(crate::count::testing::nets_of(&patches)),
                patches,
                flipped: false,
                trims,
            });
        }
        let mut order = Order::default();
        for k in 1..=8 {
            order.pos.insert((Operand::B, VertId(k)), vpos(VertId(k)));
        }
        order.pos.insert((Operand::A, VertId(1)), [0.0; 3]);
        let at = DVec3::ZERO;
        let edges: BTreeMap<EdgeId, [VertId; 2]> =
            edges.into_iter().map(|((a, b), e)| (e, [a, b])).collect();
        for (&e, &ends) in &edges {
            order.edges.insert((Operand::B, e), ends);
        }
        Cube {
            faces: out,
            order,
            edges,
            d,
            at,
        }
    }

    /// The cube with `A`'s vertex at `at` instead.
    fn with_vertex_at(mut self, at: DVec3) -> Cube {
        self.at = at;
        self.order
            .pos
            .insert((Operand::A, VertId(1)), at.to_array());
        self
    }

    fn plane(&self, f: FaceId) -> (DVec3, f64) {
        let (o, a, b) = faces()[f.0 as usize - 1];
        let [o3, a3, b3] = [o, a, b].map(|c| DVec3::from_array(c.map(f64::from)));
        let n = a3.cross(b3);
        (n, n.dot(o3))
    }
}

/// `A`'s vertex at the corner.
fn v() -> Point {
    Point::vert(Operand::A, VertId(1))
}

impl ShadowGeometry for Cube {
    fn point(&self, p: Point) -> Moving {
        match (p.operand, p.q) {
            (Operand::A, Query::Vert(VertId(1))) => Moving::of_a(self.at, Sign::Pos, self.d),
            (Operand::B, Query::Vert(w)) => Moving::fixed(DVec3::from_array(vpos(w))),
            _ => panic!("no such point {p:?}"),
        }
    }

    fn near_radius(&self, _: Point) -> f64 {
        3.0 * BAND
    }

    fn ends(&self, _: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&g]
    }

    fn line(&self, _: Point, g: EdgeId) -> Seg {
        let [a, b] = self.edges[&g].map(|v| DVec3::from_array(vpos(v)));
        Seg::fixed(a, b)
    }
}

impl FeetGeometry for Cube {
    fn curve(&self, _: Operand, g: EdgeId) -> EdgeCurve<'_> {
        EdgeCurve::Line(self.edges[&g].map(|v| DVec3::from_array(vpos(v))))
    }

    fn near_radius_of(&self, _: Operand, _: EdgeId) -> f64 {
        3.0 * BAND
    }
}

impl HeightModel for Cube {
    fn plane(&self, _: Operand, f: FaceId) -> Option<(DVec3, f64)> {
        Some(Cube::plane(self, f))
    }

    fn hit_face(&self, _: Operand, f: FaceId) -> Option<crate::records::height::hit::HitFace<'_>> {
        self.faces[f.0 as usize - 1].hit_face(&self.order)
    }

    fn plane_rates(&self, _: Point, _: FaceId, _: DVec3) -> [Rate; ORDERS] {
        [Rate::ZERO, Rate::ZERO, Rate::ZERO, Rate::ZERO]
    }

    fn foot_point(&self, _: Point, g: EdgeId, _: FootId) -> Option<Moving> {
        // The point of the edge nearest `A`'s vertex (the edges are along
        // the axes: exactly).
        let [a, b] = self.edges[&g].map(|v| DVec3::from_array(vpos(v)));
        let t = ((self.at - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
        Some(Moving::fixed(a + (b - a) * t))
    }

    fn curved_target<S: Curved>(&self, _: HKey, _: &S) -> Option<[S; 3]> {
        None
    }

    fn face_at<S: Curved>(&self, _: Point, _: FaceId, _: &[S; 3], _: &S) -> Option<FaceAt<S>> {
        None
    }
}

use crate::num::exact::ORDERS;
use crate::perturb::Curved;

/// The counts of `A`'s vertex at the corner against every cube face,
/// filling the records they read.
fn counts(cube: &Cube) -> Vec<i64> {
    counts_keys(cube).0
}

/// [`counts`] and the height keys they read.
fn counts_keys(cube: &Cube) -> (Vec<i64>, Vec<HKey>) {
    let q = v();
    let mut shadows = Shadows::new();
    let mut feet = NearFeet::new();
    let edges: Vec<EdgeId> = cube.edges.keys().copied().collect();
    let rays: Vec<RayKey> = edges.iter().map(|&g| RayKey { q, g }).collect();
    shadows
        .fill_rays(
            rays.clone(),
            cube,
            crate::num::decide::Band::curved(0.0, 1.0 / 1_073_741_824.0).expect("a band"),
            &mut Seq,
        )
        .unwrap();
    shadows
        .fill_vv((1..=8).map(|w| VvKey::of(q, VertId(w)).key), cube, &mut Seq)
        .unwrap();
    feet.fill(rays, cube, &mut Seq).unwrap();
    let reads = Records {
        shadows: &shadows,
        feet: &feet,
        order: &cube.order,
    };
    let chain_of = |t: &TrimData| {
        let rec = shadows.ray(q, t.edge).expect("filled");
        let [a, b] = t.ends.map(vpos);
        line_chain(&rec, a, b, cube.at.to_array())
    };
    let mut heights = Heights::new();
    let mut keys = Vec::new();
    let geom = ModelHeights { model: cube };
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).unwrap();
    let mut out = Vec::new();
    for f in &cube.faces {
        let p = f
            .pieces(
                &cube.order,
                q,
                cube.at.to_array(),
                &chain_of,
                &reads,
                &Sides::new(),
            )
            .unwrap_or_else(|e| panic!("face {:?}: {e:?}", f.id));
        let wanted = p.wanted(&reads, &[]).unwrap();
        keys.extend(wanted.iter().map(|k| k.h));
        heights.fill(wanted, &geom, band, &mut Seq).unwrap();
        out.push(p.count(&reads, &heights, &[]).unwrap());
    }
    (out, keys)
}

/// A random rotation, from a random unit quaternion.
fn rotation(rng: &mut Rng) -> glam::DQuat {
    loop {
        let q = glam::DQuat::from_xyzw(
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
        );
        let l = q.length();
        if l > 0.1 && l <= 1.0 {
            return q / l;
        }
    }
}

#[test]
fn faces_at_a_shared_corner_count_the_moved_point() {
    let mut rng = Rng::new(0x5c2a);
    let mut seen = [0usize; 2];
    for _ in 0..varde_testing::pick(8, 24) {
        let r = rotation(&mut rng);
        // `A`'s box at the corner along the rotated axes: its vertex
        // direction the sum of its outward normals there.
        let d = -(r * DVec3::ONE);
        let cube = Cube::new(d);
        let inside = d.x > 0.0 && d.y > 0.0 && d.z > 0.0;
        let c = counts(&cube);
        let w: i64 = c.iter().sum();
        assert_eq!(w, i64::from(inside), "d = {d:?}, counts {c:?}");
        seen[usize::from(inside)] += 1;
    }
    // Directions in and out of the cube both met.
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
}

#[test]
fn the_axis_directions_into_and_out_of_the_cube() {
    for (d, want) in [
        (DVec3::new(1.0, 2.0, 3.0), 1),
        (DVec3::new(-1.0, -2.0, -3.0), 0),
        (DVec3::new(1.0, 2.0, -3.0), 0),
        (DVec3::new(-1.0, 2.0, 3.0), 0),
    ] {
        let c = counts(&Cube::new(d));
        assert_eq!(c.iter().sum::<i64>(), want, "d = {d:?}: {c:?}");
    }
}

#[test]
fn a_corner_on_a_tile_corner_reads_the_point_vertex_record() {
    // The cube's corner at the query point on the common corner of four
    // tiles: its corner boxes lie in four tiles round it, at its height.
    let mut rng = Rng::new(0x5c2b);
    let mut seen = [0usize; 2];
    let mut dirs = vec![
        DVec3::new(1.0, 2.0, 3.0),
        DVec3::new(-1.0, -2.0, -3.0),
        DVec3::new(1.0, 2.0, -3.0),
    ];
    for _ in 0..varde_testing::pick(3, 12) {
        dirs.push(-(rotation(&mut rng) * DVec3::ONE));
    }
    for d in dirs {
        let inside = d.x > 0.0 && d.y > 0.0 && d.z > 0.0;
        let c = counts(&Cube::tiled(d, Tiling::Skew));
        assert_eq!(c.iter().sum::<i64>(), i64::from(inside), "d = {d:?}: {c:?}");
        seen[usize::from(inside)] += 1;
    }
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
}

#[test]
fn a_corner_on_a_tile_corner_with_its_sides_on_tile_lines() {
    // The square's sides at the corner run along the tile lines through
    // the query point's shadow: the strip runs beside them have outer
    // sides along their own p-curve, and every cut across a side lies on
    // a tile line, at a box corner.
    let mut rng = Rng::new(0x5c2c);
    let mut seen = [0usize; 2];
    let mut dirs = vec![
        DVec3::new(1.0, 2.0, 3.0),
        DVec3::new(-1.0, -2.0, -3.0),
        DVec3::new(1.0, 2.0, -3.0),
        DVec3::new(-1.0, 2.0, 3.0),
    ];
    for _ in 0..varde_testing::pick(3, 12) {
        dirs.push(-(rotation(&mut rng) * DVec3::ONE));
    }
    for d in dirs {
        let inside = d.x > 0.0 && d.y > 0.0 && d.z > 0.0;
        let c = counts(&Cube::tiled(d, Tiling::Lines));
        assert_eq!(c.iter().sum::<i64>(), i64::from(inside), "d = {d:?}: {c:?}");
        seen[usize::from(inside)] += 1;
    }
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
}

/// `A`'s vertex exactly on an edge of the cube, inside it: the two faces
/// at the edge read the edge's germ key `Curve(v, g, k)` (no hit's own
/// key: its value would be zero on one sheet only), decided by each
/// face's own plane at the tie, and the counts over the cube are the
/// moved point's winding, flipping with the direction.
#[test]
fn a_point_on_a_cube_edge_reads_the_edge_s_germ_key() {
    let mut rng = Rng::new(0x5c2d);
    let mut seen = [0usize; 2];
    let at = DVec3::new(0.375, 0.0, 0.0);
    let mut dirs = vec![
        DVec3::new(0.1, 1.0, 2.0),
        DVec3::new(-0.3, -1.0, 2.0),
        DVec3::new(0.2, 1.0, -0.5),
    ];
    for _ in 0..varde_testing::pick(4, 12) {
        dirs.push(rng.direction());
    }
    for d in dirs {
        for d in [d, -d] {
            let inside = d.y > 0.0 && d.z > 0.0;
            let (c, keys) = counts_keys(&Cube::new(d).with_vertex_at(at));
            assert_eq!(c.iter().sum::<i64>(), i64::from(inside), "d = {d:?}: {c:?}");
            assert!(
                keys.iter().any(|k| matches!(k, HKey::Curve { .. })),
                "d = {d:?}: {keys:?}"
            );
            seen[usize::from(inside)] += 1;
        }
    }
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
}
