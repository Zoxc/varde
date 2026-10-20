//! A test driver for the snap on the sheet fixtures
//! ([`varde_testkit::sheets`]): one snap run end to end on two stores
//! built from a fixture pair, every pass's input made from the stores,
//! nothing from the fixture's own knowledge of its shape.
//!
//! **Bodies** ([`Body`]): a fixture body built by
//! [`Brep::from_polygons`], its faces planes on their lattices' base
//! tiles, with what the passes read of it: each face's tile, exact, and
//! its loop's UV polygon.
//!
//! **Leaves** ([`leaves`]): per pair of faces whose world boxes meet
//! (closed boxes, exact), the certified flat leaves from face-sized
//! dyadic boxes down (`start_boxes`), a box dropped when it doesn't meet
//! its face's UV polygon (closed, exact) and split while its side exceeds
//! half its face's UV extent.
//!
//! One cluster's snap is in [`run`], the operation's (merges, stale
//! leaves, escalation, join) in [`op`], a zone's trim boolean on its
//! target faces in [`zone`], the comparison with the exact oracle in
//! [`oracle`].

use std::cmp::Ordering;

use malachite::Rational;
use varde_decide::bezier::{Dir, HomPatch};
use varde_decide::count::boxes::{BoxKey, Extent};
use varde_decide::ids::{EdgeId, FaceId, VertId};
use varde_decide::leaves::flat::{self, BoxUse, FlatFace, Leaf};
use varde_decide::order::{EdgeKey, EntityOrder, FaceOrd, VertKey};
use varde_decide::records::keys::Operand;
use varde_testkit::sheets::Polygons;

use crate::geom::form::FormKey;
use crate::infra::budget::{Budget, Work};
use crate::topo::brep_min::{BodyKind, Brep};

pub(crate) mod op;
pub(crate) mod oracle;
pub(crate) mod run;
pub(crate) mod zone;

type Q = Rational;

/// The face keys the driver orders by: a face's plane, no name.
pub(crate) type Key = FaceOrd<FormKey, ()>;

/// The seam target the bodies are built to (mm).
pub(crate) const TARGET: f64 = 2.5e-6;

/// A fixture body built (module docs).
pub(crate) struct Body {
    /// Which operand it is.
    pub(crate) op: Operand,
    /// The fixture data it was built from.
    pub(crate) poly: Polygons,
    /// The store.
    pub(crate) brep: Brep,
    /// Per face, its base tile exactly.
    pub(crate) tiles: Vec<HomPatch<Q>>,
    /// Per face, its loop's UV points in loop order.
    pub(crate) uv: Vec<Vec<[f64; 2]>>,
    /// Per face, its UV extent (the larger side of its UV bounding box).
    pub(crate) extent: Vec<f64>,
}

impl Body {
    /// The store of `poly` as operand `op`.
    pub(crate) fn new(op: Operand, poly: &Polygons) -> Body {
        let kind = if poly.closed {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        };
        let brep = Brep::from_polygons(
            kind,
            &poly.verts,
            &poly.faces,
            TARGET,
            &mut Work::new(&Budget::DEFAULT),
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let n = brep.face_count();
        let mut tiles = Vec::with_capacity(n);
        let mut uv = Vec::with_capacity(n);
        let mut extent = Vec::with_capacity(n);
        for f in 0..n {
            let face = FaceId(f as u32);
            let s = brep.surface(face);
            let t = s.tile(0, 0).expect("a plane's base tile");
            tiles.push(t.exact());
            let l = brep.loops(face).next().expect("an outer loop");
            let pts: Vec<[f64; 2]> = brep
                .loop_coedges(l)
                .map(|c| brep.coedge_uv(c, false).expect("a p-curve"))
                .collect();
            let mut lo = [f64::INFINITY; 2];
            let mut hi = [f64::NEG_INFINITY; 2];
            for p in &pts {
                for k in 0..2 {
                    lo[k] = if p[k] < lo[k] { p[k] } else { lo[k] };
                    hi[k] = if p[k] > hi[k] { p[k] } else { hi[k] };
                }
            }
            let w = hi[0] - lo[0];
            let h = hi[1] - lo[1];
            extent.push(if w > h { w } else { h });
            uv.push(pts);
        }
        Body {
            op,
            poly: poly.clone(),
            brep,
            tiles,
            uv,
            extent,
        }
    }

    /// Face `f`'s key: its plane's.
    pub(crate) fn face_key(&self, f: FaceId) -> Key {
        FaceOrd {
            form: Some(*self.brep.surface(f).lattice().key()),
            name: (),
        }
    }

    /// Face `f`'s world box: its vertices' bounding box.
    fn world_box(&self, f: FaceId) -> [[f64; 3]; 2] {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for &v in &self.poly.faces[f.0 as usize] {
            let p = self.brep.pos(VertId(v));
            for k in 0..3 {
                lo[k] = if p[k] < lo[k] { p[k] } else { lo[k] };
                hi[k] = if p[k] > hi[k] { p[k] } else { hi[k] };
            }
        }
        [lo, hi]
    }

    /// The boxes face `f`'s leaves start from: the dyadic boxes of the
    /// finest level whose side holds the face's UV extent that cover its
    /// UV bounding box (at most two by two). Every box the whole tile
    /// splits into that meets the face lies in one of them, so the leaves
    /// are those of the whole tile, found without its upper levels.
    fn start_boxes(&self, f: usize) -> Vec<BoxKey> {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in &self.uv[f] {
            for k in 0..2 {
                lo[k] = if p[k] < lo[k] { p[k] } else { lo[k] };
                hi[k] = if p[k] > hi[k] { p[k] } else { hi[k] };
            }
        }
        let mut d = 0u32;
        while d < 52 && f64::from_bits(((1023 - d as i64 - 1) as u64) << 52) >= self.extent[f] {
            d += 1;
        }
        let scale = f64::from_bits(((1023 + d as i64) as u64) << 52);
        let range = |k: usize| {
            let a = (lo[k] * scale).floor() as u64;
            let b = ((hi[k] * scale).floor() as u64).min((1u64 << d) - 1);
            a..=b
        };
        let ext = |i: u64| {
            Extent::new(
                varde_decide::bezier::Dyadic::new(i, d).expect("a dyadic"),
                varde_decide::bezier::Dyadic::new(i + 1, d).expect("a dyadic"),
            )
            .expect("an extent")
        };
        let mut out = Vec::new();
        for i in range(0) {
            for j in range(1) {
                out.push(BoxKey::new(
                    self.op,
                    FaceId(f as u32),
                    [0, 0],
                    ext(i),
                    ext(j),
                ));
            }
        }
        out
    }

    /// What [`flat::leaves`] does with box `b` of this body (module docs).
    fn use_box(&self, b: &BoxKey) -> BoxUse {
        let f = b.face().0 as usize;
        let [u, v] = [Dir::U, Dir::V].map(|d| b.extent(d));
        let rect = [u, v].map(|e: Extent| [e.lo().rational(), e.hi().rational()]);
        let tile = b.tile().map(Q::from);
        let rect = [0, 1].map(|k| [&tile[k] + &rect[k][0], &tile[k] + &rect[k][1]]);
        if !rect_meets_polygon(&rect, &self.uv[f]) {
            return BoxUse::Drop;
        }
        let side = &rect[0][1] - &rect[0][0];
        if side > q(self.extent[f]) / Q::from(2u32) {
            BoxUse::Split
        } else {
            BoxUse::Test
        }
    }
}

/// The exact value of a finite `f64`.
pub(crate) fn q(x: f64) -> Q {
    Q::try_from(x).expect("finite")
}

/// The sign of `(b − a) × (c − a)`, exactly.
fn orient(a: &[Q; 2], b: &[Q; 2], c: &[Q; 2]) -> Ordering {
    let d = (&b[0] - &a[0]) * (&c[1] - &a[1]) - (&b[1] - &a[1]) * (&c[0] - &a[0]);
    d.cmp(&Q::from(0))
}

/// Whether the closed rectangle `r` (`[[u_lo, u_hi], [v_lo, v_hi]]`)
/// meets the closed polygon `poly`, exactly.
pub(crate) fn rect_meets_polygon(r: &[[Q; 2]; 2], poly: &[[f64; 2]]) -> bool {
    let pts: Vec<[Q; 2]> = poly.iter().map(|p| [q(p[0]), q(p[1])]).collect();
    let corners = [
        [r[0][0].clone(), r[1][0].clone()],
        [r[0][1].clone(), r[1][0].clone()],
        [r[0][1].clone(), r[1][1].clone()],
        [r[0][0].clone(), r[1][1].clone()],
    ];
    let n = pts.len();
    for k in 0..n {
        let (a, b) = (&pts[k], &pts[(k + 1) % n]);
        // The segment's box against the rectangle.
        let apart = (0..2).any(|i| {
            let (lo, hi) = if a[i] < b[i] {
                (&a[i], &b[i])
            } else {
                (&b[i], &a[i])
            };
            *hi < r[i][0] || *lo > r[i][1]
        });
        if apart {
            continue;
        }
        // The rectangle's corners all strictly on one side of the line.
        let s: Vec<Ordering> = corners.iter().map(|c| orient(a, b, c)).collect();
        let all = |o: Ordering| s.iter().all(|&x| x == o);
        if !all(Ordering::Less) && !all(Ordering::Greater) {
            return true;
        }
    }
    // No edge meets it: inside the polygon or outside, as its corner is.
    inside(&corners[0], &pts)
}

/// Whether `p` lies inside the polygon `pts` (not on it), by the
/// crossings of the ray from `p` along `+u`, exactly.
fn inside(p: &[Q; 2], pts: &[[Q; 2]]) -> bool {
    let n = pts.len();
    let mut odd = false;
    for k in 0..n {
        let (a, b) = (&pts[k], &pts[(k + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            // The crossing's u: a + (p.v − a.v)/(b.v − a.v)·(b − a).
            let t = (&p[1] - &a[1]) / (&b[1] - &a[1]);
            let u = &a[0] + t * (&b[0] - &a[0]);
            if u > p[0] {
                odd = !odd;
            }
        }
    }
    odd
}

/// Whether two closed world boxes meet.
fn boxes_meet(a: &[[f64; 3]; 2], b: &[[f64; 3]; 2]) -> bool {
    (0..3).all(|k| a[0][k] <= b[1][k] && b[0][k] <= a[1][k])
}

/// The gap bound of the snap at the default tolerance, `ε/2 = 5·10⁻⁴`.
pub(crate) const B_K: f64 = 5e-4;

/// Every leaf of `a` against `b` (module docs), face pair by face pair in
/// face order.
pub(crate) fn leaves(a: &Body, b: &Body, b_k: f64) -> Vec<Leaf> {
    let mut out = Vec::new();
    let bk = q(b_k);
    for fa in 0..a.tiles.len() {
        let wa = a.world_box(FaceId(fa as u32));
        for fb in 0..b.tiles.len() {
            let wb = b.world_box(FaceId(fb as u32));
            if !boxes_meet(&wa, &wb) {
                continue;
            }
            for a0 in a.start_boxes(fa) {
                for b0 in b.start_boxes(fb) {
                    let (ls, _) = flat::leaves(
                        FlatFace {
                            patch: &a.tiles[fa],
                            flipped: a.brep.flipped(FaceId(fa as u32)),
                        },
                        a0,
                        FlatFace {
                            patch: &b.tiles[fb],
                            flipped: b.brep.flipped(FaceId(fb as u32)),
                        },
                        b0,
                        &bk,
                        1 << 22,
                        |bx| match bx.operand() {
                            Operand::A => a.use_box(bx),
                            Operand::B => b.use_box(bx),
                        },
                    )
                    .unwrap_or_else(|e| panic!("faces {fa}, {fb}: {e:?}"));
                    out.extend(ls);
                }
            }
        }
    }
    out
}

/// The two bodies' vertices and edges in the geometric order, edges
/// straight between their stored ends.
pub(crate) struct Order<'a> {
    pub(crate) a: &'a Body,
    pub(crate) b: &'a Body,
}

impl Order<'_> {
    pub(crate) fn body(&self, o: Operand) -> &Body {
        match o {
            Operand::A => self.a,
            Operand::B => self.b,
        }
    }
}

impl EntityOrder for Order<'_> {
    type Key = ();

    fn ends(&self, o: Operand, g: EdgeId) -> [VertId; 2] {
        self.body(o).brep.ends(g)
    }

    fn vert_key(&self, o: Operand, v: VertId) -> VertKey<()> {
        VertKey::new(self.body(o).brep.pos(v), []).expect("finite")
    }

    fn edge_key(&self, o: Operand, g: EdgeId) -> EdgeKey<()> {
        let [a, b] = self.ends(o, g);
        let br = &self.body(o).brep;
        let (p, r) = (br.pos(a), br.pos(b));
        let mid = [0, 1, 2].map(|k| 0.5 * (p[k] + r[k]));
        EdgeKey::new([self.vert_key(o, a), self.vert_key(o, b)], mid, [(), ()]).expect("finite")
    }
}

#[cfg(test)]
mod tests;
