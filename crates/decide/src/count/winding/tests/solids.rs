//! End to end on two boxes: `B` a cube, `A` a box turned, scaled and
//! moved at random to overlap it, vertex and edge ids shuffled. Every
//! record the counting reads is decided once: the ray records of each
//! operand's vertices against the other's edges, the point–vertex
//! records and the feet. From them each face count `s02` (`s20` for `B`'s
//! vertices) through the face's pieces ([`crate::count::s02`]), each edge
//! pair's crossing record through its clusters ([`clusters`]) with
//! `s11` from their z-order, `x12` and `x21` by identity
//! ([`crate::count::cross`]), the seeds by clearance from the other
//! operand's boxes, and the windings carried from them. Checked against
//! the geometry: each crossing count is the edge's signed crossings with
//! the face, each winding whether the vertex lies inside the other box,
//! and equals its own column's count. The faces are plane tiles whose
//! column hits are exact, so a piece near a point reads its hits' height
//! records, decided on them. Placements within `GAP` of a tie are skipped;
//! a crossing whose z-order isn't clear reads its edge–edge height record,
//! exact on the edges' segments, and agrees with the geometry's.

use glam::{DQuat, DVec3};

use super::*;
use crate::count::cross::{Coedge, x12, x21};
use crate::count::s02::line_chain;
use crate::count::testing::{FaceData, Patch, Seq, TrimData, first, line, nets_of, pline, whole};
use crate::count::xrec::clusters;
use crate::ids::{CoedgeId, FaceId};
use crate::num::decide::Band;
use crate::num::decide::Sign;
use crate::num::exact::ORDERS;
use crate::perturb::{Curved, Moving, NU, R_HAT, Rate, Seg, UP, rates_a};
use crate::records::feet::{EdgeCurve, FeetGeometry, NearFeet};
use crate::records::height::hit::HitFace;
use crate::records::height::model::{FaceAt, HeightModel, ModelHeights};
use crate::records::height::{Heights, Records};
use crate::records::keys::{FootId, HKey, RayKey, VvKey, XKey};
use crate::records::shadow::{ShadowGeometry, Shadows};
use crate::records::side::Sides;
use crate::records::xrec::{EdgeRays, XCurve, XPair, XSides, XTerms, ZKey, x_count, zorder_key};

use Operand::{A, B};

/// Each trim's band, and the near radii (three bands).
const BAND: f64 = 1.0 / 4096.0;

/// Closer than this to a tie (relative to the boxes' unit size), a
/// placement is skipped: well past the near radii, so no column meets a
/// face near its point.
const GAP: f64 = 4e-3;

/// The size a box is cleared by from the other operand.
const EPS_TIE: f64 = 1e-9;

/// The patch's map: local `s ∈ [−0.3, 1.4]` over the tile, so the face's
/// square `[0, 1]²` is trimmed inside it at non-dyadic parameters.
const LO: f64 = -0.3;
const SPAN: f64 = 1.7;

/// A box's faces by corner bits: origin, `a`, `b` with `a × b` outward.
const FACES: [([u8; 3], [u8; 3], [u8; 3]); 6] = [
    ([0, 0, 0], [0, 1, 0], [1, 0, 0]),
    ([0, 0, 1], [1, 0, 0], [0, 1, 0]),
    ([0, 0, 0], [0, 0, 1], [0, 1, 0]),
    ([1, 0, 0], [0, 1, 0], [0, 0, 1]),
    ([0, 0, 0], [1, 0, 0], [0, 0, 1]),
    ([0, 1, 0], [0, 0, 1], [1, 0, 0]),
];

/// A box: corner `o`, edges `axes` (right-handed, orthogonal), its
/// vertices by id, edges by id (lower-id end first) and faces as
/// loops of vertex ids, outward.
struct Solid {
    op: Operand,
    o: DVec3,
    axes: [DVec3; 3],
    pos: Vec<DVec3>,
    edges: Vec<[VertId; 2]>,
    loops: Vec<[VertId; 4]>,
    faces: Vec<FaceData>,
}

fn bits(c: [u8; 3]) -> usize {
    usize::from(c[0]) + 2 * usize::from(c[1]) + 4 * usize::from(c[2])
}

impl Solid {
    fn new(rng: &mut Rng, op: Operand, o: DVec3, axes: [DVec3; 3]) -> Solid {
        let vperm = perm(rng, 8);
        let mut pos = vec![DVec3::ZERO; 8];
        for (c, &id) in vperm.iter().enumerate() {
            let b = [c & 1, (c >> 1) & 1, (c >> 2) & 1].map(|x| x as f64);
            pos[id] = o + axes[0] * b[0] + axes[1] * b[1] + axes[2] * b[2];
        }
        let vid = |c: [u8; 3]| VertId(vperm[bits(c)] as u32);
        let add = |p: [u8; 3], q: [u8; 3]| [0, 1, 2].map(|k| p[k] + q[k]);
        let loops: Vec<[VertId; 4]> = FACES
            .iter()
            .map(|&(o, a, b)| [o, add(o, a), add(add(o, a), b), add(o, b)].map(vid))
            .collect();
        let mut ends: Vec<[VertId; 2]> = loops
            .iter()
            .flat_map(|l| (0..4).map(move |k| sorted(l[k], l[(k + 1) % 4])))
            .collect();
        ends.sort_unstable();
        ends.dedup();
        let eperm = perm(rng, ends.len());
        let mut edges = vec![[VertId(0); 2]; ends.len()];
        for (k, &e) in ends.iter().enumerate() {
            edges[eperm[k]] = e;
        }
        let mut s = Solid {
            op,
            o,
            axes,
            pos,
            edges,
            loops,
            faces: Vec::new(),
        };
        s.faces = (0..6).map(|f| s.face(f)).collect();
        s
    }

    fn edge(&self, u: VertId, v: VertId) -> EdgeId {
        let k = sorted(u, v);
        EdgeId(self.edges.iter().position(|&e| e == k).expect("an edge") as u32)
    }

    fn at(&self, v: VertId) -> [f64; 3] {
        self.pos[v.index()].to_array()
    }

    /// Face `f` as the counting reads it: a planar patch over one tile,
    /// its square trimmed inside at non-dyadic parameters.
    fn face(&self, f: usize) -> FaceData {
        let l = self.loops[f];
        let [p0, p1, _, p3] = l.map(|v| self.pos[v.index()]);
        let (a3, b3) = (p1 - p0, p3 - p0);
        let mut net = Vec::new();
        for i in 0..3 {
            for j in 0..3 {
                let [s, t] = [i, j].map(|k| LO + SPAN * f64::from(k) / 2.0);
                let p = p0 + a3 * s + b3 * t;
                net.push([p.x, p.y, p.z, 1.0]);
            }
        }
        let patch: Patch = Patch::from_affine(2, 2, net, &first()).expect("a patch");
        let st = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let uv = |s: [f64; 2]| s.map(|x| (x - LO) / SPAN);
        let trims = (0..4)
            .map(|k| {
                let (p, q) = (l[k], l[(k + 1) % 4]);
                let ends = sorted(p, q);
                let (s0, s1) = if p == ends[0] {
                    (st[k], st[(k + 1) % 4])
                } else {
                    (st[(k + 1) % 4], st[k])
                };
                TrimData {
                    coedge: CoedgeId(4 * f as u32 + k as u32),
                    edge: self.edge(p, q),
                    reversed: p != ends[0],
                    spans: vec![pline(uv(s0), uv(s1))],
                    ends,
                    band: BAND,
                    curve: vec![line(self.at(ends[0]), self.at(ends[1]))],
                    maps: whole(1),
                }
            })
            .collect();
        let patches = vec![patch];
        FaceData {
            operand: self.op,
            id: FaceId(f as u32),
            tiles: [0..1, 0..1],
            nets: Some(nets_of(&patches)),
            patches,
            flipped: false,
            trims,
        }
    }

    /// Face `f`'s coedges.
    fn boundary(&self, f: usize) -> Vec<Coedge> {
        let l = self.loops[f];
        (0..4)
            .map(|k| Coedge {
                edge: self.edge(l[k], l[(k + 1) % 4]),
                forward: l[k] < l[(k + 1) % 4],
            })
            .collect()
    }

    /// `p`'s coordinates in the box's frame, `[0, 1]³` inside.
    fn local(&self, p: DVec3) -> [f64; 3] {
        self.axes.map(|a| a.dot(p - self.o) / a.length_squared())
    }

    /// Whether `p` lies inside, `None` within `GAP` of the boundary.
    fn inside(&self, p: DVec3) -> Option<bool> {
        let x = self.local(p);
        if x.iter().any(|&c| c.abs() < GAP || (c - 1.0).abs() < GAP) {
            return None;
        }
        Some(x.iter().all(|&c| (0.0..=1.0).contains(&c)))
    }

    /// The signed crossings of the segment `a → b` with face `f`
    /// (`−sign(C′·n_f)` each, entering `+1`), `None` near a tie.
    fn crossings(&self, a: DVec3, b: DVec3, f: usize) -> Option<i64> {
        let l = self.loops[f];
        let [p0, p1, _, p3] = l.map(|v| self.pos[v.index()]);
        let (u, v) = (p1 - p0, p3 - p0);
        let n = u.cross(v).normalize();
        let (ha, hb) = (n.dot(a - p0), n.dot(b - p0));
        if ha.abs() < GAP || hb.abs() < GAP {
            return None;
        }
        if (ha > 0.0) == (hb > 0.0) {
            return Some(0);
        }
        let x = a + (b - a) * (ha / (ha - hb)) - p0;
        let st = [u.dot(x) / u.length_squared(), v.dot(x) / v.length_squared()];
        if st.iter().any(|&c| c.abs() < GAP || (c - 1.0).abs() < GAP) {
            return None;
        }
        let inside = st.iter().all(|&c| (0.0..=1.0).contains(&c));
        Some(if !inside {
            0
        } else if n.dot(b - a) < 0.0 {
            1
        } else {
            -1
        })
    }

    fn graph(&self) -> Graph<'_> {
        Graph {
            operand: self.op,
            verts: self.pos.len() as u32,
            edges: &self.edges,
        }
    }
}

fn sorted(u: VertId, v: VertId) -> [VertId; 2] {
    if u < v { [u, v] } else { [v, u] }
}

/// The two boxes, `A` moving along `d`.
struct World {
    a: Solid,
    b: Solid,
    d: DVec3,
    order: Order,
}

impl World {
    fn solid(&self, op: Operand) -> &Solid {
        match op {
            A => &self.a,
            B => &self.b,
        }
    }

    fn pos(&self, p: Point) -> DVec3 {
        self.solid(p.operand).pos[vert(&p).index()]
    }

    /// Whether `e`'s shadow crosses `g`'s with `g` above at the crossing,
    /// `None` if they don't cross; `Err` near a tie.
    fn above(&self, e: EdgeId, g: EdgeId) -> Result<Option<Sign>, ()> {
        let [a, b] = self.a.edges[e.index()].map(|v| self.a.pos[v.index()]);
        let [g0, g1] = self.b.edges[g.index()].map(|v| self.b.pos[v.index()]);
        let sh = |p: DVec3| [R_HAT.normalize().dot(p), NU.normalize().dot(p)];
        let ([ar, av], [br, bv], [gr, gv], [hr, hv]) = (sh(a), sh(b), sh(g0), sh(g1));
        // a + t(b − a) = g0 + s(g1 − g0) in the shadow.
        let (m00, m01, m10, m11) = (br - ar, gr - hr, bv - av, gv - hv);
        let det = m00 * m11 - m01 * m10;
        let (r0, r1) = (gr - ar, gv - av);
        if det.abs() < GAP {
            return if r0.abs() < GAP && r1.abs() < GAP {
                Err(())
            } else {
                Ok(None)
            };
        }
        let t = (r0 * m11 - m01 * r1) / det;
        let s = (m00 * r1 - r0 * m10) / det;
        let near = |x: f64| x.abs() < GAP || (x - 1.0).abs() < GAP;
        if near(t) || near(s) {
            return Err(());
        }
        if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&s) {
            return Ok(None);
        }
        let gap = UP.normalize().dot((g0 + (g1 - g0) * s) - (a + (b - a) * t));
        if gap.abs() < GAP {
            return Err(());
        }
        Ok(Some(if gap > 0.0 { Sign::Pos } else { Sign::Neg }))
    }
}

impl ShadowGeometry for World {
    fn point(&self, p: Point) -> Moving {
        match p.operand {
            A => Moving::of_a(self.pos(p), Sign::Pos, self.d),
            B => Moving::fixed(self.pos(p)),
        }
    }

    fn near_radius(&self, _: Point) -> f64 {
        3.0 * BAND
    }

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.solid(operand).edges[g.index()]
    }

    fn line(&self, q: Point, g: EdgeId) -> Seg {
        let s = self.solid(q.operand.other());
        let [a, b] = s.edges[g.index()].map(|v| s.pos[v.index()]);
        match s.op {
            A => Seg::of_a(a, b, Sign::Pos, self.d),
            B => Seg::fixed(a, b),
        }
    }
}

impl FeetGeometry for World {
    fn curve(&self, operand: Operand, g: EdgeId) -> EdgeCurve<'_> {
        let s = self.solid(operand);
        EdgeCurve::Line(s.edges[g.index()].map(|v| s.pos[v.index()]))
    }

    fn near_radius_of(&self, _: Operand, _: EdgeId) -> f64 {
        3.0 * BAND
    }
}

/// The planes by their faces' frames and outward normals, straight edges
/// at the point nearest the asking one's column.
impl HeightModel for World {
    fn plane(&self, operand: Operand, f: FaceId) -> Option<(DVec3, f64)> {
        let s = self.solid(operand);
        let [p0, p1, _, p3] = s.loops[f.index()].map(|v| s.pos[v.index()]);
        let n = (p1 - p0).cross(p3 - p0);
        Some((n, n.dot(p0)))
    }

    fn plane_rates(&self, q: Point, _: FaceId, _: DVec3) -> [Rate; ORDERS] {
        match q.operand.other() {
            A => rates_a(Sign::Pos, self.d),
            B => [Rate::ZERO, Rate::ZERO, Rate::ZERO, Rate::ZERO],
        }
    }

    fn hit_face(&self, operand: Operand, f: FaceId) -> Option<HitFace<'_>> {
        self.solid(operand).faces[f.index()].hit_face(&self.order)
    }

    fn edge_seg(&self, operand: Operand, g: EdgeId) -> Option<Seg> {
        let s = self.solid(operand);
        let [a, b] = s.edges[g.index()].map(|v| s.pos[v.index()]);
        Some(match operand {
            A => Seg::of_a(a, b, Sign::Pos, self.d),
            B => Seg::fixed(a, b),
        })
    }

    fn foot_point(&self, q: Point, g: EdgeId, _: FootId) -> Option<Moving> {
        let s = self.solid(q.operand.other());
        let [a, b] = s.edges[g.index()].map(|v| s.pos[v.index()]);
        let x = self.pos(q);
        let t = ((x - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
        let at = a + (b - a) * t;
        Some(match s.op {
            A => Moving::of_a(at, Sign::Pos, self.d),
            B => Moving::fixed(at),
        })
    }

    fn curved_target<S: Curved>(&self, _: HKey, _: &S) -> Option<[S; 3]> {
        None
    }

    fn face_at<S: Curved>(&self, _: Point, _: FaceId, _: &[S; 3], _: &S) -> Option<FaceAt<S>> {
        None
    }
}

/// A random rotation, from a random unit quaternion.
fn rotation(rng: &mut Rng) -> DQuat {
    loop {
        let q = DQuat::from_xyzw(
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

fn world(rng: &mut Rng) -> World {
    let b = Solid::new(
        rng,
        B,
        DVec3::ZERO,
        [DVec3::X, DVec3::Y, DVec3::Z].map(|a| 2.0 * a),
    );
    let r = rotation(rng);
    let size = rng.range(1.0, 2.5);
    let o = DVec3::new(
        rng.range(-1.0, 2.5),
        rng.range(-1.0, 2.5),
        rng.range(-1.0, 2.5),
    );
    let a = Solid::new(
        rng,
        A,
        o,
        [DVec3::X, DVec3::Y, DVec3::Z].map(|x| r * x * size),
    );
    let d = rng.direction();
    let mut order = Order::default();
    for s in [&a, &b] {
        for (v, p) in (0u32..).zip(&s.pos) {
            order.pos.insert((s.op, VertId(v)), p.to_array());
        }
        for (e, &ends) in (0u32..).zip(&s.edges) {
            order.edges.insert((s.op, EdgeId(e)), ends);
        }
    }
    World { a, b, d, order }
}

/// What the geometry says: per vertex of each operand whether it lies
/// inside the other, per edge and face of the other the crossings, per
/// edge pair the z-order where the shadows cross. `None` near a tie.
struct Truth {
    inside: [Vec<bool>; 2],
    /// `[x12, x21]`, by edge then face.
    cross: [Vec<Vec<i64>>; 2],
    above: BTreeMap<(EdgeId, EdgeId), Sign>,
}

fn truth(w: &World) -> Option<Truth> {
    let mut inside = [Vec::new(), Vec::new()];
    let mut cross = [Vec::new(), Vec::new()];
    for (k, (s, o)) in [(&w.a, &w.b), (&w.b, &w.a)].into_iter().enumerate() {
        for &p in &s.pos {
            inside[k].push(o.inside(p)?);
        }
        for ends in &s.edges {
            let [a, b] = ends.map(|v| s.pos[v.index()]);
            let row: Option<Vec<i64>> = (0..6).map(|f| o.crossings(a, b, f)).collect();
            cross[k].push(row?);
        }
    }
    let mut above = BTreeMap::new();
    for e in 0..12 {
        for g in 0..12 {
            if let Some(z) = w.above(EdgeId(e), EdgeId(g)).ok()? {
                above.insert((EdgeId(e), EdgeId(g)), z);
            }
        }
    }
    Some(Truth {
        inside,
        cross,
        above,
    })
}

/// The records of a run: every ray, point–vertex and feet record either
/// operand's vertices read against the other.
struct Recs {
    shadows: Shadows,
    feet: NearFeet,
}

fn records(w: &World) -> Recs {
    let mut rays = Vec::new();
    let mut vv = Vec::new();
    for (s, o) in [(&w.a, &w.b), (&w.b, &w.a)] {
        for v in 0..s.pos.len() as u32 {
            let q = Point::vert(s.op, VertId(v));
            rays.extend((0..o.edges.len() as u32).map(|g| RayKey { q, g: EdgeId(g) }));
            vv.extend((0..o.pos.len() as u32).map(|x| VvKey::of(q, VertId(x)).key));
        }
    }
    let mut shadows = Shadows::new();
    shadows.fill_rays(rays.clone(), w, &mut Seq).expect("rays");
    shadows
        .fill_vv(vv, w, &mut Seq)
        .expect("point–vertex records");
    let mut feet = NearFeet::new();
    feet.fill(rays, w, &mut Seq).expect("feet");
    Recs { shadows, feet }
}

/// `s02(v, f)` for every vertex `v` of `s` and face `f` of `o` (`s20` for
/// `B`'s vertices), by vertex then face.
fn face_counts(w: &World, r: &Recs, s: &Solid, o: &Solid) -> Vec<Vec<i64>> {
    let reads = Records {
        shadows: &r.shadows,
        feet: &r.feet,
        order: &w.order,
    };
    let mut heights = Heights::new();
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).expect("a band");
    let corners: Vec<_> = o.faces.iter().map(|f| f.corners(&w.order)).collect();
    let mut out = Vec::new();
    for v in 0..s.pos.len() as u32 {
        let q = Point::vert(s.op, VertId(v));
        let at = s.pos[v as usize].to_array();
        let chain = |t: &TrimData| {
            let rec = r.shadows.ray(q, t.edge).expect("decided");
            let [a, b] = t.ends.map(|x| o.at(x));
            line_chain(&rec, a, b, at)
        };
        let mut row = Vec::new();
        for (f, c) in o.faces.iter().zip(&corners) {
            let p = f
                .pieces_with(Some(c), &w.order, q, at, &chain, &reads, &Sides::new())
                .unwrap_or_else(|e| panic!("{q:?} face {:?}: {e:?}", f.id));
            let wanted = p.wanted(&reads, &[]).expect("hit keys");
            heights
                .fill(wanted, &ModelHeights { model: w }, band, &mut Seq)
                .expect("heights");
            row.push(p.count(&reads, &heights, &[]).expect("a count"));
        }
        out.push(row);
    }
    out
}

/// `s11(e, g)` for every edge pair: the clusters of its crossing record,
/// each unclear z-order read from its height record (and checked against
/// the geometry's).
fn s11s(w: &World, r: &Recs, t: &Truth) -> BTreeMap<(EdgeId, EdgeId), i64> {
    let band = Band::curved(0.0, 1.0 / 1_073_741_824.0).expect("a band");
    let mut heights = Heights::new();
    let curve = |s: &Solid, e: EdgeId| {
        let [a, b] = s.edges[e.index()].map(|v| s.at(v));
        XCurve::Line(a, b)
    };
    let ray = |q: Point, g: EdgeId| EdgeRays::Line(r.shadows.ray(q, g).expect("decided"));
    let mut out = BTreeMap::new();
    for e in (0..w.a.edges.len() as u32).map(EdgeId) {
        for g in (0..w.b.edges.len() as u32).map(EdgeId) {
            let [a, b] = w.a.edges[e.index()].map(|v| Point::vert(A, v));
            let [g0, g1] = w.b.edges[g.index()].map(|v| Point::vert(B, v));
            let pair = XPair {
                e: curve(&w.a, e),
                g: curve(&w.b, g),
                terms: XTerms {
                    a_g: ray(a, g),
                    b_g: ray(b, g),
                    g0_e: ray(g0, e),
                    g1_e: ray(g1, e),
                },
                shared: false,
            };
            // Straight edges: one cell with outer sides only, no side
            // record read.
            let out_c = clusters(&w.order, XKey { e, g }, &pair, &XSides::new(), &first())
                .unwrap_or_else(|err| panic!("{e:?} {g:?}: {err:?}"));
            assert_eq!(out_c.rec.degree(), x_count(&pair.terms));
            let z = t.above.get(&(e, g)).copied();
            assert_eq!(
                out_c.rec.degree().abs(),
                i64::from(z.is_some()),
                "{e:?} {g:?}"
            );
            let s = out_c
                .rec
                .s11(|c, _| {
                    let k = zorder_key(XKey { e, g }, c);
                    heights
                        .fill(
                            [k.height_key(g)],
                            &ModelHeights { model: w },
                            band,
                            &mut Seq,
                        )
                        .expect("a z-order record");
                    let read = k.above(&heights.get(k.key, ZKey::anchor(g)).expect("decided"));
                    assert_eq!(Some(read), z, "{e:?} {g:?}");
                    Ok::<_, ()>(read)
                })
                .expect("s11");
            out.insert((e, g), s);
        }
    }
    out
}

/// The windings of `s`'s vertices: seeded at its first cleared vertex
/// against `o`'s boxes, by `counts` there, carried by `x`'s rows summed.
fn windings_of(
    w: &World,
    s: &Solid,
    o: &Solid,
    counts: &[Vec<i64>],
    x: &[Vec<i64>],
) -> (Seed, Vec<i64>) {
    let hulls: Vec<_> = o
        .faces
        .iter()
        .map(|f| f.patches[0].hull_box().expect("a hull"))
        .collect();
    let (r, spent) = seeds(&w.order, &s.graph(), 64, |v| {
        (Ok::<_, ()>(clear_of(s.at(v), EPS_TIE, &hulls)), 1)
    });
    let seeds = r.expect("seeds");
    assert_eq!(seeds.len(), 1);
    assert!((1..=8).contains(&spent));
    let s0 = seeds[0];
    let w0: i64 = counts[s0.vert.index()].iter().sum();
    let ws = windings(&w.order, &s.graph(), &[(s0.vert, w0)], |e| {
        Ok::<_, ()>(x[e.index()].iter().sum())
    })
    .expect("windings");
    (s0, ws)
}

#[test]
fn windings_from_the_records_match_the_geometry() {
    let mut rng = Rng::new(0x3a5);
    let (mut trials, mut skipped, mut crossing, mut inside) = (0, 0, 0, 0);
    while trials < varde_testing::pick(6, 40) {
        let w = world(&mut rng);
        let Some(t) = truth(&w) else {
            skipped += 1;
            assert!(skipped < 100, "too many placements near a tie");
            continue;
        };
        trials += 1;
        let r = records(&w);
        let s02 = face_counts(&w, &r, &w.a, &w.b);
        let s20 = face_counts(&w, &r, &w.b, &w.a);
        let s11 = s11s(&w, &r, &t);
        // `x12(e, f)` for every edge of `A` and face of `B`, `x21(g, f)`
        // for every edge of `B` and face of `A`.
        let x12s: Vec<Vec<i64>> = (0..w.a.edges.len())
            .map(|e| {
                let [a, b] = w.a.edges[e];
                (0..6)
                    .map(|f| {
                        x12(
                            [s02[a.index()][f], s02[b.index()][f]],
                            &w.b.boundary(f),
                            |g| Ok::<_, ()>(s11[&(EdgeId(e as u32), g)]),
                        )
                        .expect("x12")
                    })
                    .collect()
            })
            .collect();
        let x21s: Vec<Vec<i64>> = (0..w.b.edges.len())
            .map(|g| {
                let [g0, g1] = w.b.edges[g];
                (0..6)
                    .map(|f| {
                        x21(
                            [s20[g0.index()][f], s20[g1.index()][f]],
                            &w.a.boundary(f),
                            |e| Ok::<_, ()>(s11[&(e, EdgeId(g as u32))]),
                        )
                        .expect("x21")
                    })
                    .collect()
            })
            .collect();
        assert_eq!(x12s, t.cross[0], "x12");
        assert_eq!(x21s, t.cross[1], "x21");
        crossing += x12s
            .iter()
            .chain(&x21s)
            .flatten()
            .filter(|&&x| x != 0)
            .count();
        for (k, (s, o, counts, x)) in [(&w.a, &w.b, &s02, &x12s), (&w.b, &w.a, &s20, &x21s)]
            .into_iter()
            .enumerate()
        {
            let (seed, ws) = windings_of(&w, s, o, counts, x);
            // Every vertex is clear of the other's faces by `GAP`.
            assert!(seed.cleared);
            let want: Vec<i64> = t.inside[k].iter().map(|&b| i64::from(b)).collect();
            assert_eq!(ws, want, "operand {:?}", s.op);
            // Each vertex's own column agrees.
            let own: Vec<i64> = counts.iter().map(|row| row.iter().sum()).collect();
            assert_eq!(own, want, "operand {:?}", s.op);
            inside += want.iter().filter(|&&w| w == 1).count();
        }
    }
    assert!(
        crossing > 0 && inside > 0,
        "{crossing} crossings, {inside} inside, {skipped} skipped"
    );
}
