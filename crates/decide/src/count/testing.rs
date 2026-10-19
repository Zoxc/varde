//! Test support shared by the counting's tests: test faces of planar or
//! curved patches with their trims ([`FaceData`]; a plane face with its
//! exact frame, whose column hits are exact), counted against a point
//! through [`face_pieces`]; the geometric order of test operands
//! ([`Order`]); a pass deciding records in order on one thread ([`Seq`]);
//! straight spans and p-spans.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::bezier::HomPatch;
use crate::count::boxes::{BoxError, Corners, FaceUv, Trim, UvSpan};
use crate::count::pieces::{Ribbon, SpanMap};
use crate::count::s02::{
    Charge, CountError, FacePieces, FaceQuery, TrimEdge, face_pieces, ray_degree,
};
use crate::ids::{CoedgeId, EdgeId, FaceId, VertId};
use crate::num::bounded::Bounded;
use crate::num::decide::Fail;
use crate::num::decide::Sign;
use crate::order::{Along, EdgeKey, EntityOrder, VertKey};
use crate::perturb::{R_HAT, rate_rational, rates_a};
use crate::records::height::HeightReads;
use crate::records::height::hit::{HitFace, HitTrim, Motion};
use crate::records::keys::{Operand, Point};
use crate::records::ray::{ChainRec, Span, TileNet};
use crate::records::shadow::{Cost, Decided, Pass};
use crate::records::side::Sides;
use crate::records::table::Fresh;

pub(crate) type Patch = HomPatch<Bounded<f64>>;

/// The stored net of tile `[i, j]` of a face whose point at the global
/// `(u, v)` is `map(u, v)`, an affine map: its nine points at the tile's
/// half steps, weights 1.
pub(crate) fn mapped_net([i, j]: [i64; 2], map: impl Fn(f64, f64) -> [f64; 3]) -> TileNet {
    std::array::from_fn(|k| {
        let (u, v) = (
            i as f64 + (k / 3) as f64 / 2.0,
            j as f64 + (k % 3) as f64 / 2.0,
        );
        let [x, y, z] = map(u, v);
        [x, y, z, 1.0]
    })
}

/// A face query at the first level.
pub(crate) type FirstQuery<'a> = FaceQuery<'a, 'a, Bounded<f64>>;

pub(crate) fn first() -> Bounded<f64> {
    Bounded::exact(0.0)
}

/// A pass deciding in order on one thread.
pub(crate) struct Seq;

impl Pass for Seq {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        keys.iter().map(|k| decide(k).0).collect()
    }
}

/// One trim of a test face, with its edge's curve.
#[derive(Clone, Debug)]
pub(crate) struct TrimData {
    pub(crate) coedge: CoedgeId,
    pub(crate) edge: EdgeId,
    pub(crate) reversed: bool,
    /// The p-curve, along the edge's stored direction.
    pub(crate) spans: Vec<UvSpan>,
    /// The edge's stored ends.
    pub(crate) ends: [VertId; 2],
    pub(crate) band: f64,
    /// The edge's curve, along its stored direction.
    pub(crate) curve: Vec<Span>,
    pub(crate) maps: Vec<SpanMap>,
}

/// A test face.
#[derive(Clone, Debug)]
pub(crate) struct FaceData {
    pub(crate) operand: Operand,
    pub(crate) id: FaceId,
    pub(crate) tiles: [Range<i64>; 2],
    pub(crate) patches: Vec<Patch>,
    pub(crate) flipped: bool,
    pub(crate) trims: Vec<TrimData>,
    /// A plane face's tiles' nets, one per patch (its column hits exact
    /// on them); `None` for a curved face.
    pub(crate) nets: Option<Vec<TileNet>>,
}

/// The geometric order of a test operand's vertices and edges.
#[derive(Clone, Debug, Default)]
pub(crate) struct Order {
    pub(crate) pos: BTreeMap<(Operand, VertId), [f64; 3]>,
    pub(crate) edges: BTreeMap<(Operand, EdgeId), [VertId; 2]>,
}

impl EntityOrder for Order {
    type Key = ();

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(operand, g)]
    }

    fn vert_key(&self, operand: Operand, v: VertId) -> VertKey<()> {
        VertKey::new(self.pos[&(operand, v)], []).expect("no NaN")
    }

    fn edge_key(&self, operand: Operand, g: EdgeId) -> EdgeKey<()> {
        let [s, e] = self.edges[&(operand, g)];
        let [a, b] = [s, e].map(|v| self.pos[&(operand, v)]);
        let mid = [0, 1, 2].map(|k| 0.5 * (a[k] + b[k]));
        EdgeKey::new([s, e].map(|v| self.vert_key(operand, v)), mid, [(), ()]).expect("no NaN")
    }
}

impl FaceData {
    /// The face as its hits read it; `None` for a face given no nets.
    pub(crate) fn hit_face(&self, order: &Order) -> Option<HitFace<'_>> {
        let nets = self.nets.as_deref()?;
        let trims = self
            .trims
            .iter()
            .map(|t| {
                let [s, e] = t.ends;
                let along = Along::of(
                    &order.vert_key(self.operand, s),
                    &order.vert_key(self.operand, e),
                );
                HitTrim {
                    coedge: t.coedge,
                    edge: t.edge,
                    reversed: t.reversed,
                    ends: t.ends,
                    along,
                    band: t.band,
                    pspans: &t.spans,
                    curve: &t.curve,
                    map: &t.maps,
                }
            })
            .collect();
        Some(HitFace {
            operand: self.operand,
            face: self.id,
            tiles: self.tiles.clone(),
            nets,
            inside: if self.flipped { -1 } else { 1 },
            trims,
        })
    }

    /// The face's vertices in geometric order.
    pub(crate) fn order(&self, order: &Order) -> Vec<VertId> {
        let mut vs: Vec<VertId> = self.trims.iter().flat_map(|t| t.ends).collect();
        vs.sort_unstable();
        vs.dedup();
        let mut keyed: Vec<_> = vs
            .into_iter()
            .map(|v| (order.vert_key(self.operand, v), v))
            .collect();
        keyed.sort();
        keyed.into_iter().map(|(_, v)| v).collect()
    }

    /// The face's pieces against `point` at `at`, the ray records of its
    /// edges from `rays`.
    pub(crate) fn pieces<R: HeightReads>(
        &self,
        order: &Order,
        point: Point,
        at: [f64; 3],
        rays: &dyn Fn(&TrimData) -> ChainRec,
        reads: &R,
        sides: &Sides,
    ) -> Result<FacePieces, CountError> {
        self.pieces_with(None, order, point, at, rays, reads, sides)
    }

    /// The face's corners, to count it against many points
    /// ([`FaceData::pieces_with`]).
    pub(crate) fn corners(&self, order: &Order) -> Corners {
        self.corners_on(order, &Charge::setup(u64::MAX))
            .0
            .expect("corners")
    }

    /// The face's corners built on `charge`, with what they spent.
    pub(crate) fn corners_on(
        &self,
        order: &Order,
        charge: &Charge,
    ) -> (Result<Corners, BoxError>, Cost) {
        let trims = self.uv_trims();
        Corners::new(&self.uv(&trims), &self.patches, &self.order(order), charge)
    }

    /// [`FaceData::pieces`] with the face's `corners` if given (else
    /// built here).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn pieces_with<R: HeightReads>(
        &self,
        corners: Option<&Corners>,
        order: &Order,
        point: Point,
        at: [f64; 3],
        rays: &dyn Fn(&TrimData) -> ChainRec,
        reads: &R,
        sides: &Sides,
    ) -> Result<FacePieces, CountError> {
        self.query_with(corners, order, point, at, rays, &|fq, edges| {
            let out = face_pieces(fq, sides, reads)?;
            assert_eq!(
                out.degree(),
                ray_degree(fq.face, edges).unwrap(),
                "the pieces' degrees sum to the ray records'"
            );
            Ok(out)
        })
    }

    /// `f` of the face's query against `point` at `at` and its trims'
    /// edges, the ray records of its edges from `rays`.
    pub(crate) fn query<T>(
        &self,
        order: &Order,
        point: Point,
        at: [f64; 3],
        rays: &dyn Fn(&TrimData) -> ChainRec,
        f: &dyn Fn(&FirstQuery<'_>, &[TrimEdge<'_>]) -> T,
    ) -> T {
        self.query_with(None, order, point, at, rays, f)
    }

    /// The face's trims as its UV domain reads them.
    fn uv_trims(&self) -> Vec<Trim<'_>> {
        self.trims
            .iter()
            .map(|t| Trim {
                coedge: t.coedge,
                reversed: t.reversed,
                spans: &t.spans,
                ends: t.ends,
                zero_length: false,
                band: t.band,
            })
            .collect()
    }

    /// The face's UV domain over `trims` ([`FaceData::uv_trims`]).
    fn uv<'t>(&self, trims: &'t [Trim<'t>]) -> FaceUv<'t> {
        self.uv_scaled(trims, [1.0, 1.0])
    }

    /// [`FaceData::uv`] with the splits comparing the sides at `scale`
    /// (another partition of the face).
    fn uv_scaled<'t>(&self, trims: &'t [Trim<'t>], scale: [f64; 2]) -> FaceUv<'t> {
        FaceUv::new(
            self.operand,
            self.id,
            self.tiles.clone(),
            scale,
            trims,
            self.flipped,
        )
        .expect("a valid face")
    }

    /// [`FaceData::query`] with the face's `corners` if given (else built
    /// here).
    pub(crate) fn query_with<T>(
        &self,
        corners: Option<&Corners>,
        order: &Order,
        point: Point,
        at: [f64; 3],
        rays: &dyn Fn(&TrimData) -> ChainRec,
        f: &dyn Fn(&FirstQuery<'_>, &[TrimEdge<'_>]) -> T,
    ) -> T {
        self.query_scaled([1.0, 1.0], corners, order, point, at, rays, f)
    }

    /// [`FaceData::query_with`] with the splits comparing the box sides
    /// at `scale`: another partition of the face.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn query_scaled<T>(
        &self,
        scale: [f64; 2],
        corners: Option<&Corners>,
        order: &Order,
        point: Point,
        at: [f64; 3],
        rays: &dyn Fn(&TrimData) -> ChainRec,
        f: &dyn Fn(&FirstQuery<'_>, &[TrimEdge<'_>]) -> T,
    ) -> T {
        let trims = self.uv_trims();
        let face = self.uv_scaled(&trims, scale);
        let built;
        let corners = match corners {
            Some(c) => c,
            None => {
                built = Corners::new(
                    &face,
                    &self.patches,
                    &self.order(order),
                    &Charge::setup(u64::MAX),
                )
                .0
                .expect("corners");
                &built
            }
        };
        let recs: Vec<ChainRec> = self.trims.iter().map(rays).collect();
        let edges: Vec<TrimEdge<'_>> = self
            .trims
            .iter()
            .zip(&recs)
            .map(|(t, r)| TrimEdge {
                edge: t.edge,
                ribbon: Ribbon {
                    coedge: t.coedge,
                    curve: &t.curve,
                    map: &t.maps,
                    rays: r,
                },
            })
            .collect();
        let fq = FaceQuery {
            face: &face,
            corners,
            patches: &self.patches,
            edges: &edges,
            point,
            at,
            eps_tie: 1e-9,
            nets: self.nets.as_deref(),
            motion: &motion_of(point.operand),
        };
        f(&fq, &edges)
    }
}

/// The test faces' motion against a point of `of` ([`Motion`]): a point
/// of `A` moves along `r̂` (then the fixed `T₂ … T₄`), `B` stays.
pub(crate) fn motion_of(of: Operand) -> Motion {
    let rates = rates_a(Sign::Pos, R_HAT);
    std::array::from_fn(|n| {
        let r = rate_rational(&rates[n]);
        match of {
            Operand::A => r.map(|c| -c),
            Operand::B => r,
        }
    })
}

/// The nets of patches as `Patch::from_affine` takes them, back from
/// the patches' exact first-level values (weights one).
pub(crate) fn nets_of(patches: &[Patch]) -> Vec<TileNet> {
    patches
        .iter()
        .map(|p| {
            let [x, y, z, w] = p.tensors().each_ref().map(|t| t.coefs());
            std::array::from_fn(|k| {
                let wk = w[k].v;
                [x[k].v / wk, y[k].v / wk, z[k].v / wk, wk]
            })
        })
        .collect()
}

/// A straight span from `a` to `b`.
pub(crate) fn line(a: [f64; 3], b: [f64; 3]) -> Span {
    let m = [0, 1, 2].map(|k| 0.5 * (a[k] + b[k]));
    [
        [a[0], a[1], a[2], 1.0],
        [m[0], m[1], m[2], 1.0],
        [b[0], b[1], b[2], 1.0],
    ]
}

/// A straight p-span.
pub(crate) fn pline(a: [f64; 2], b: [f64; 2]) -> UvSpan {
    [
        [a[0], a[1], 1.0],
        [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 1.0],
        [b[0], b[1], 1.0],
    ]
}

pub(crate) fn whole(n: u32) -> Vec<SpanMap> {
    (0..n)
        .map(|span| SpanMap {
            span,
            t: [0.0, 1.0],
        })
        .collect()
}
