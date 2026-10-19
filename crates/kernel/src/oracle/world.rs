//! Two solids counted against each other: every record the counting
//! reads decided once on the stored bodies (point–vertex, ray against
//! straight and curved edges, feet), each face count `s02` (`s20` for
//! `B`'s vertices) through its pieces at the precision levels it needs,
//! each edge pair's crossing record through its clusters and `s11`,
//! `x12` and `x21` by identity, the seeds and the windings carried from
//! them. `A` moves along a constant direction `d` (`σ_A = +1`).
//!
//! Every face's column hits are exact, plane or curved (its stored tiles,
//! trims and edges as the hits read them), the face's germ keys' rows
//! too; a vertex whose winding the hits leave outside
//! `{0, 1}` within an edge's near radius takes the lobe fallback there
//! (the edge rule for its hits near the edge), counted.
//!
//! What the counting can't answer yet is a [`Refusal`] with its cause,
//! never a guess: a hit the exact hits don't form (a fold they don't
//! decide, a column in a planar tile), a height key of a curved edge
//! away from exact incidence (the lobe fallback's edge rule there: no
//! curved height model), a crossing cluster whose z-order record runs
//! past the last precision level (every unclear cluster reads its
//! edge–edge or point–curve record, decided on the crossing record's
//! clusters: exact on straight edges, at the levels on curved ones), two
//! straight edges with collinear shadows not one sign in height over
//! their overlap, a side record a piece asks for, a record or count that
//! runs past the last precision level, a ladder signal from the windings.

use std::cmp::Ordering;

use glam::DVec3;
use malachite::Rational;
use malachite::base::num::arithmetic::traits::Sign as _;
use malachite::base::num::basic::traits::{One as _, Zero as _};
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode::Nearest;
use varde_decide::bezier::Dir;
use varde_decide::bezier::{HomPatch, HullBox};
use varde_decide::count::boxes::{BoxError, Corners, FaceUv, Trim};
use varde_decide::count::cross::{Coedge, x12, x21};
use varde_decide::count::pieces::{Ribbon, SpanMap};
use varde_decide::count::s02::FacePieces;
use varde_decide::count::s02::{Charge, CountError, FaceQuery, TrimEdge, count_pieces, line_chain};
use varde_decide::count::winding::{Graph, Lobes, WindingError, clear_of, seeds, windings_lobed};
use varde_decide::count::xrec::{XClusterError, form};
use varde_decide::ids::{CoedgeId, EdgeId, FaceId, VertId};
use varde_decide::num::bounded::Bounded;
use varde_decide::num::decide::{Band, Fail, Sign};
use varde_decide::num::exact::ORDERS;
use varde_decide::order::{Along, EdgeKey, EntityOrder, VertKey};
use varde_decide::perturb::{Curved, Moving, Rate, Seg, T, rate_rational, rates_a};
use varde_decide::records::feet::{EdgeCurve, FeetGeometry, NearFeet};
use varde_decide::records::height::hit::{HitFace, HitTrim, Motion};
use varde_decide::records::height::model::{FaceAt, HeightModel, ModelHeights, XAt, XHeights};
use varde_decide::records::height::{HeightError, Heights, Records};
use varde_decide::records::keys::{EdgeAt, Operand, Point, Query, RayKey, VvKey, XKey};
use varde_decide::records::keys::{FootId, HKey};
use varde_decide::records::keys::{SideLine, TileCorner, TileEdge, TilePoint};
use varde_decide::records::near::OpBands;
use varde_decide::records::ray::{ChainRec, CurveGeometry, CurvedRays, Span, TileNet};
use varde_decide::records::shadow::{Cost, Decided, LEVELS, Pass, ShadowGeometry, Shadows};
use varde_decide::records::side::{SideGeometry, Sides};
use varde_decide::records::table::Fresh;
use varde_decide::records::xrec::{
    EdgeRays, XBase, XCurve, XGeometry, XPair, XRec, XSides, XTerms, ZKey, zorder_key,
};

use super::solids::Solid;

use Operand::{A, B};

/// The counting's settings for a run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Config {
    /// The size a core box near a point refines to (mm), and the seeds'
    /// clearance.
    pub eps_tie: f64,
    /// The curved records' error target (mm): a curved value certified
    /// within it of 0 is a tie.
    pub target: f64,
    /// Factors on each face's UV unit lengths, which the box splits
    /// compare to pick a direction: another partition of the same face.
    pub scale: [f64; 2],
}

impl Config {
    /// The runs' settings: `ε_tie` an eighth of a `10⁻⁵` tolerance; the
    /// curved records decide by value down to `2⁻³⁰` mm (about `10⁻⁹`),
    /// far below the plan's `ε_tie/4` so that only true ties (and values
    /// within `10⁻⁹` of one) are perturbed, yet above what the first
    /// levels can reach on a tile row `4·10⁶` mm long (a curved root's
    /// parameter is an `f64`, `2⁻⁵³` of the row, ~`5·10⁻¹⁰` mm).
    pub(super) const RUNS: Config = Config {
        eps_tie: 1.25e-6,
        target: 1.0 / 1_073_741_824.0,
        scale: [1.0, 1.0],
    };
}

/// Why a placement has no counted answer.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Cause {
    /// A record pass failed: `kind` names the record, the failure its
    /// [`Fail`] (escalated past the last level, or the guard).
    Record { kind: &'static str, fail: String },
    /// A face count ran out ([`Exhausted`]).
    OutOfBudget(String),
    /// A face count's side record can't be decided, or it kept asking.
    Side(String),
    /// A face count's hit near the point reads a height record the model
    /// can't decide (a curved edge's away from exact incidence: no curved
    /// height model here), or a face's hits aren't formed.
    Height,
    /// A face count failed otherwise.
    Count(String),
    /// An edge pair's clusters can't be formed at any level.
    Clusters(String),
    /// A crossing cluster's z-order can't be decided: its height record
    /// (edge–edge or point–curve) runs past the last level, or two
    /// straight edges' collinear shadows aren't one sign in height over
    /// their overlap (no cluster reads that until the crossings along it
    /// are found).
    ZOrder,
    /// Edges on one curve whose `h = ê·(T × Δ)` changes sign along their
    /// overlap: the moved edges cross there (refused with `Crossed`,
    /// never a wrong answer).
    Crossed,
    /// The seeds can't be found.
    Seeds(String),
    /// The windings disagree with the crossings (`Disagree`) or leave
    /// `{0, 1}` (`Outside`): a ladder signal.
    Signal(String),
    /// Another failure.
    Other(String),
}

/// A refusal with where it happened.
#[derive(Clone, Debug)]
pub(super) struct Refusal {
    pub cause: Cause,
    pub at: String,
}

/// What a placement's counting ran: the records decided per kind and the
/// highest precision level each reached, the face counts' and the
/// clusters' levels.
#[derive(Clone, Debug, Default)]
pub(super) struct Stats {
    /// Per record kind (point–vertex, flat ray, curved ray span/knot/
    /// whole edge, feet): records decided, and how many reached each
    /// level (the highest level with curved work; exact-only records at
    /// level 0), and how many ran exact rational arithmetic.
    pub records: std::collections::BTreeMap<&'static str, KindStats>,
    /// Face counts per highest level reached.
    pub counts: [u64; LEVELS],
    /// Edge pairs' clusters per level formed at.
    pub clusters: [u64; LEVELS],
    /// Side records filled at a count's request.
    pub side_fills: u64,
    /// Edge pairs' side and corner records filled at the clusters'
    /// request.
    pub x_side_fills: u64,
    /// Face counts whose hits read height records.
    pub height_fills: u64,
    /// Lobe fallbacks taken: `(q, g)` whose hits near `g` were re-read
    /// by the edge rule.
    pub lobes: u64,
    /// Edge pairs on one curve.
    pub shared: u64,
    /// Every edge pair's refusal, by cause (not only a placement's
    /// first): the rate of `Crossed` among shared pairs.
    pub pair_causes: std::collections::BTreeMap<String, u64>,
    /// Clusters whose z-order was clear.
    pub clear: u64,
    /// Clusters whose z-order wasn't: read from their height records.
    pub zorder_records: u64,
    /// Seeds certified clear of the other operand.
    pub cleared_seeds: u64,
    /// Seeds not cleared (the first vertex taken).
    pub uncleared_seeds: u64,
    /// Seconds spent: records, face counts (with their corners),
    /// clusters.
    pub time: [f64; 3],
    /// Seconds spent building the faces' corners.
    pub corners: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct KindStats {
    pub decided: u64,
    pub levels: [u64; LEVELS],
    pub rationals: u64,
}

impl Stats {
    pub(super) fn add(&mut self, o: &Stats) {
        for (k, s) in &o.records {
            let e = self.records.entry(k).or_default();
            e.decided += s.decided;
            e.rationals += s.rationals;
            for l in 0..LEVELS {
                e.levels[l] += s.levels[l];
            }
        }
        for l in 0..LEVELS {
            self.counts[l] += o.counts[l];
            self.clusters[l] += o.clusters[l];
        }
        self.clear += o.clear;
        self.shared += o.shared;
        for (k, n) in &o.pair_causes {
            *self.pair_causes.entry(k.clone()).or_default() += n;
        }
        self.side_fills += o.side_fills;
        self.x_side_fills += o.x_side_fills;
        self.height_fills += o.height_fills;
        self.lobes += o.lobes;
        self.zorder_records += o.zorder_records;
        self.cleared_seeds += o.cleared_seeds;
        for k in 0..3 {
            self.time[k] += o.time[k];
        }
        self.corners += o.corners;
        self.uncleared_seeds += o.uncleared_seeds;
    }
}

/// The highest level with curved work in `c` (0 for exact work alone).
fn top_level(c: &Cost) -> usize {
    (0..LEVELS)
        .rev()
        .find(|&l| {
            let w = &c.levels[l];
            w.evals > 0 || w.roots.splits > 0 || w.roots.tests > 0 || w.roots.steps > 0
        })
        .unwrap_or(0)
}

/// A pass deciding in order on one thread, tallying what it decided.
struct Tally<'s> {
    kind: &'static str,
    stats: &'s mut Stats,
}

impl Pass for Tally<'_> {
    type Error = Fail;

    fn run<K: Sync, V: Send>(
        &mut self,
        keys: &[Fresh<K>],
        decide: &(dyn Fn(&Fresh<K>) -> Decided<V> + Sync),
    ) -> Result<Vec<V>, Fail> {
        let s = self.stats.records.entry(self.kind).or_default();
        keys.iter()
            .map(|k| {
                let (v, c) = decide(k);
                s.decided += 1;
                s.levels[top_level(&c)] += 1;
                if c.exact.rationals > 0 {
                    s.rationals += 1;
                }
                v
            })
            .collect()
    }
}

/// The geometric order of both operands' vertices and edges.
struct Order {
    pos: [Vec<[f64; 3]>; 2],
    ends: [Vec<[VertId; 2]>; 2],
    mid: [Vec<[f64; 3]>; 2],
}

fn ix(op: Operand) -> usize {
    match op {
        A => 0,
        B => 1,
    }
}

impl EntityOrder for Order {
    type Key = ();

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.ends[ix(operand)][g.index()]
    }

    fn vert_key(&self, operand: Operand, v: VertId) -> VertKey<()> {
        VertKey::new(self.pos[ix(operand)][v.index()], []).expect("no NaN")
    }

    fn edge_key(&self, operand: Operand, g: EdgeId) -> EdgeKey<()> {
        let [s, e] = self.ends(operand, g);
        EdgeKey::new(
            [self.vert_key(operand, s), self.vert_key(operand, e)],
            self.mid[ix(operand)][g.index()],
            [(), ()],
        )
        .expect("no NaN")
    }
}

/// One trim of a face as its hits read it, beside the store's
/// spans.
struct HitTrimData {
    coedge: CoedgeId,
    edge: EdgeId,
    reversed: bool,
    ends: [VertId; 2],
    along: Along,
    band: f64,
    map: Vec<SpanMap>,
}

/// A face, plane or curved, as its hits read it.
struct HitData {
    tiles: [std::ops::Range<i64>; 2],
    nets: Vec<TileNet>,
    inside: i64,
    trims: Vec<HitTrimData>,
}

/// The two solids as the records read them.
pub(super) struct World<'s> {
    solids: [&'s Solid; 2],
    d: DVec3,
    bands: OpBands,
    vert_near: [Vec<f64>; 2],
    order: Order,
    /// Per operand and face, its hit data.
    flats: [Vec<Option<HitData>>; 2],
}

impl<'s> World<'s> {
    pub(super) fn new(a: &'s Solid, b: &'s Solid, d: DVec3) -> World<'s> {
        let (ma, mb) = (a.brep.max_coord(), b.brep.max_coord());
        let m = if ma > mb { ma } else { mb };
        let bands = OpBands::new(m).expect("finite coordinates");
        let solids = [a, b];
        let vert_near = solids.map(|s| s.brep.vert_near_radii(&bands).expect("radii"));
        let order = Order {
            pos: solids.map(|s| s.verts().into_iter().map(|v| s.brep.pos(v)).collect()),
            ends: solids.map(|s| s.edges().into_iter().map(|e| s.brep.ends(e)).collect()),
            mid: solids.map(|s| {
                s.edges()
                    .into_iter()
                    .map(|e| {
                        let [t0, t1] = s.brep.t_range(e);
                        let p = s.brep.curve(e).eval(0.5 * (t0 + t1)).expect("a point");
                        p.map(|c| c.v)
                    })
                    .collect()
            }),
        };
        let mut w = World {
            solids,
            d,
            bands,
            vert_near,
            order,
            flats: [Vec::new(), Vec::new()],
        };
        w.flats = [A, B].map(|op| {
            let s = w.solid(op);
            s.faces().into_iter().map(|f| w.hit_data(op, f)).collect()
        });
        w
    }

    /// Face `f` of `op`'s hit data, plane or curved; `None` for one whose
    /// p-curves don't fit their edges' spans.
    fn hit_data(&self, op: Operand, f: FaceId) -> Option<HitData> {
        let b = &self.solid(op).brep;
        let surf = b.surface(f);
        let mut trims = Vec::new();
        for l in b.loops(f) {
            for c in b.loop_coedges(l) {
                let e = b.edge(c);
                let map = span_map(b.pcurve(c).knots(), b.curve(e).knots(), b.t_range(e))?;
                let [s0, s1] = b.ends(e);
                trims.push(HitTrimData {
                    coedge: c,
                    edge: e,
                    reversed: b.reversed(c),
                    ends: [s0, s1],
                    along: Along::of(&self.order.vert_key(op, s0), &self.order.vert_key(op, s1)),
                    band: self.band(op, e),
                    map,
                });
            }
        }
        let r = surf.rect();
        let mut nets = Vec::new();
        for i in r.u.clone() {
            for j in r.v.clone() {
                nets.push(*surf.tile(i, j)?.net());
            }
        }
        Some(HitData {
            tiles: [r.u.clone(), r.v.clone()],
            nets,
            inside: if b.flipped(f) { -1 } else { 1 },
            trims,
        })
    }

    fn solid(&self, op: Operand) -> &'s Solid {
        self.solids[ix(op)]
    }

    fn straight(&self, op: Operand, g: EdgeId) -> bool {
        self.solid(op).straight[g.index()]
    }

    fn edge_points(&self, op: Operand, g: EdgeId) -> [[f64; 3]; 2] {
        let b = &self.solid(op).brep;
        b.ends(g).map(|v| b.pos(v))
    }

    /// Edge `g`'s band `β_g`.
    fn band(&self, op: Operand, g: EdgeId) -> f64 {
        let b = &self.solid(op).brep;
        self.bands.band(b.edge_tol(g)).expect("a band")
    }

    fn rates(&self, op: Operand) -> [Rate; ORDERS] {
        match op {
            A => rates_a(Sign::Pos, Rate::Float(self.d)),
            B => std::array::from_fn(|_| Rate::ZERO),
        }
    }
}

/// An operand's windings, its crossing counts and the lobe fallbacks taken.
type Windings = (Vec<i64>, Vec<Vec<i64>>, u64);

fn vert(p: Point) -> VertId {
    match p.q {
        Query::Vert(v) => v,
        Query::Cut(..) => panic!("no cut points here"),
    }
}

impl ShadowGeometry for World<'_> {
    fn point(&self, p: Point) -> Moving {
        let at = DVec3::from_array(self.solid(p.operand).brep.pos(vert(p)));
        match p.operand {
            A => Moving::of_a(at, Sign::Pos, Rate::Float(self.d)),
            B => Moving::fixed(at),
        }
    }

    fn near_radius(&self, p: Point) -> f64 {
        self.vert_near[ix(p.operand)][vert(p).index()]
    }

    fn ends(&self, operand: Operand, g: EdgeId) -> [VertId; 2] {
        self.solid(operand).brep.ends(g)
    }

    fn line(&self, q: Point, g: EdgeId) -> Seg {
        let op = q.operand.other();
        let [a, b] = self.edge_points(op, g).map(DVec3::from_array);
        match op {
            A => Seg::of_a(a, b, Sign::Pos, Rate::Float(self.d)),
            B => Seg::fixed(a, b),
        }
    }
}

impl CurveGeometry for World<'_> {
    fn spans(&self, operand: Operand, g: EdgeId) -> &[Span] {
        self.solid(operand).brep.edge_spans(g)
    }

    fn knot_rates(&self, q: Point, _: EdgeId, _: u32) -> [Rate; ORDERS] {
        self.rates(q.operand.other())
    }

    fn span_rates<S: Curved>(&self, q: Point, _: EdgeId, _: u32, t: &S) -> [[S; 3]; ORDERS] {
        let lit = |v: DVec3| [v.x, v.y, v.z].map(|x| S::from_f64(x, t));
        match q.operand.other() {
            A => [lit(self.d), lit(T[0]), lit(T[1]), lit(T[2])],
            B => std::array::from_fn(|_| lit(DVec3::ZERO)),
        }
    }
}

impl XGeometry for World<'_> {
    fn x_curve(&self, operand: Operand, g: EdgeId) -> XCurve<'_> {
        if self.straight(operand, g) {
            let [p, q] = self.edge_points(operand, g);
            XCurve::Line(p, q)
        } else {
            XCurve::Spans(self.solid(operand).brep.edge_spans(g))
        }
    }

    fn edge_point_rates(&self, operand: Operand, _: EdgeId, _: EdgeAt) -> [Rate; ORDERS] {
        self.rates(operand)
    }

    fn edge_rates<S: Curved>(
        &self,
        operand: Operand,
        _: EdgeId,
        _: u32,
        t: &S,
    ) -> [[S; 3]; ORDERS] {
        let lit = |v: DVec3| [v.x, v.y, v.z].map(|x| S::from_f64(x, t));
        match operand {
            A => [lit(self.d), lit(T[0]), lit(T[1]), lit(T[2])],
            B => std::array::from_fn(|_| lit(DVec3::ZERO)),
        }
    }

    fn edge_line(&self, operand: Operand, g: EdgeId, _: DVec3) -> Seg {
        let [a, b] = self.edge_points(operand, g).map(DVec3::from_array);
        match operand {
            A => Seg::of_a(a, b, Sign::Pos, Rate::Float(self.d)),
            B => Seg::fixed(a, b),
        }
    }
}

impl FeetGeometry for World<'_> {
    fn curve(&self, operand: Operand, g: EdgeId) -> EdgeCurve<'_> {
        if self.straight(operand, g) {
            EdgeCurve::Line(self.edge_points(operand, g).map(DVec3::from_array))
        } else {
            let [s, e] = self.ends(operand, g);
            EdgeCurve::Spans {
                spans: self.solid(operand).brep.edge_spans(g),
                closed: s == e,
            }
        }
    }

    fn near_radius_of(&self, operand: Operand, g: EdgeId) -> f64 {
        let b = &self.solid(operand).brep;
        b.edge_near_radius(g, &self.bands).expect("a radius")
    }
}

impl SideGeometry for World<'_> {
    fn tile_edge(&self, e: &TileEdge) -> Option<Span> {
        let s = self.solid(e.operand);
        let surf = s.brep.surface(e.face);
        let r = surf.rect();
        let (u, v) = (&r.u, &r.v);
        // The tile on the line's upper side if the face has it, else the
        // lower; its row along the line.
        match e.fixed {
            Dir::U => {
                if !v.contains(&e.cell) {
                    return None;
                }
                let (i, at) = if u.contains(&e.line) {
                    (e.line, 0)
                } else if u.contains(&(e.line - 1)) {
                    (e.line - 1, 2)
                } else {
                    return None;
                };
                let n = surf.tile(i, e.cell)?.net();
                Some([n[3 * at], n[3 * at + 1], n[3 * at + 2]])
            }
            Dir::V => {
                if !u.contains(&e.cell) {
                    return None;
                }
                let (j, at) = if v.contains(&e.line) {
                    (e.line, 0)
                } else if v.contains(&(e.line - 1)) {
                    (e.line - 1, 2)
                } else {
                    return None;
                };
                let n = surf.tile(e.cell, j)?.net();
                Some([n[at], n[3 + at], n[6 + at]])
            }
        }
    }

    fn corner(&self, _: Point, c: &TileCorner) -> Option<Moving> {
        let s = self.solid(c.operand);
        let surf = s.brep.surface(c.face);
        let r = surf.rect();
        let [cu, cv] = c.at;
        let pick = |x: i64, range: &std::ops::Range<i64>| {
            if range.contains(&x) {
                Some((x, 0))
            } else if range.contains(&(x - 1)) {
                Some((x - 1, 2))
            } else {
                None
            }
        };
        let ((i, a), (j, b)) = (pick(cu, &r.u)?, pick(cv, &r.v)?);
        let p = surf.tile(i, j)?.net()[3 * a + b];
        let at = DVec3::new(p[0], p[1], p[2]);
        Some(match c.operand {
            A => Moving::of_a(at, Sign::Pos, Rate::Float(self.d)),
            B => Moving::fixed(at),
        })
    }

    fn tile_net(&self, operand: Operand, face: FaceId, [i, j]: [i64; 2]) -> Option<TileNet> {
        let surf = self.solid(operand).brep.surface(face);
        let r = surf.rect();
        if !r.u.contains(&i) || !r.v.contains(&j) {
            return None;
        }
        Some(*surf.tile(i, j)?.net())
    }

    fn point_rates(&self, q: Point, _: &TilePoint) -> [Rate; ORDERS] {
        match q.operand.other() {
            A => rates_a(Sign::Pos, Rate::Float(self.d)),
            B => [Rate::ZERO, Rate::ZERO, Rate::ZERO, Rate::ZERO],
        }
    }

    fn line_rates<S: Curved>(&self, q: Point, _: &SideLine, t: &S) -> [[S; 3]; ORDERS] {
        let lit = |v: DVec3| [v.x, v.y, v.z].map(|x| S::from_f64(x, t));
        match q.operand.other() {
            A => [lit(self.d), lit(T[0]), lit(T[1]), lit(T[2])],
            B => std::array::from_fn(|_| lit(DVec3::ZERO)),
        }
    }
}

/// The height model: every face by its stored tiles (column hits and
/// the germ keys' rows on a curved face, exactly), a plane face's keys
/// by its plane (the face's outward normal), straight edges at the point
/// whose shadow is nearest the asking point's and as segments (edge–edge
/// keys). A curved edge's key away from exact incidence has no model
/// here: it escalates and the count is refused.
impl HeightModel for World<'_> {
    fn plane(&self, operand: Operand, f: FaceId) -> Option<(DVec3, f64)> {
        let b = &self.solid(operand).brep;
        plane_of(b, f)
    }

    fn plane_rates(&self, q: Point, _: FaceId, _: DVec3) -> [Rate; ORDERS] {
        self.rates(q.operand.other())
    }

    fn hit_face(&self, operand: Operand, f: FaceId) -> Option<HitFace<'_>> {
        let d = self.flats[ix(operand)].get(f.index())?.as_ref()?;
        let b = &self.solid(operand).brep;
        Some(HitFace {
            operand,
            face: f,
            tiles: d.tiles.clone(),
            nets: &d.nets,
            inside: d.inside,
            trims: d
                .trims
                .iter()
                .map(|t| HitTrim {
                    coedge: t.coedge,
                    edge: t.edge,
                    reversed: t.reversed,
                    ends: t.ends,
                    along: t.along,
                    band: t.band,
                    pspans: b.pcurve(t.coedge).spans(),
                    curve: b.edge_spans(t.edge),
                    map: &t.map,
                })
                .collect(),
        })
    }

    fn edge_spans(&self, operand: Operand, g: EdgeId) -> Option<&[Span]> {
        Some(self.solid(operand).brep.edge_spans(g))
    }

    fn edge_seg(&self, operand: Operand, g: EdgeId) -> Option<Seg> {
        if !self.straight(operand, g) {
            return None;
        }
        let [a, b] = self.edge_points(operand, g).map(DVec3::from_array);
        Some(match operand {
            A => Seg::of_a(a, b, Sign::Pos, Rate::Float(self.d)),
            B => Seg::fixed(a, b),
        })
    }

    fn foot_point(&self, q: Point, g: EdgeId, _: FootId) -> Option<Moving> {
        let op = q.operand.other();
        if !self.straight(op, g) {
            return None;
        }
        let [a, b] = self.edge_points(op, g).map(DVec3::from_array);
        let at = DVec3::from_array(self.solid(q.operand).brep.pos(vert(q)));
        let x = foot_at(a, b, at);
        Some(match op {
            A => Moving::of_a(x, Sign::Pos, Rate::Float(self.d)),
            B => Moving::fixed(x),
        })
    }

    fn curved_target<S: Curved>(&self, _: HKey, _: &S) -> Option<[S; 3]> {
        None
    }

    fn face_at<S: Curved>(&self, _: Point, _: FaceId, _: &[S; 3], _: &S) -> Option<FaceAt<S>> {
        None
    }
}

/// The point of segment `ab` whose shadow along `UP` is nearest `q`'s,
/// as an `f64` point whose height above `q`, `UP·(x − q)`, has the sign
/// of the exact foot's: the exact foot where it is an `f64` point (so a
/// `q` on the segment is its own foot, a tie), else the foot rounded,
/// moved along `z` by ulps where rounding flipped or zeroed that sign.
fn foot_at(a: DVec3, b: DVec3, q: DVec3) -> DVec3 {
    type R3 = [Rational; 3];
    let r = |p: DVec3| -> R3 { p.to_array().map(|c| Rational::try_from(c).expect("finite")) };
    let dot = |x: &R3, y: &R3| -> Rational { (0..3).map(|k| &x[k] * &y[k]).sum() };
    let sub = |x: &R3, y: &R3| -> R3 { std::array::from_fn(|k| &x[k] - &y[k]) };
    let (up, ra, rb, rq) = (r(varde_decide::perturb::UP), r(a), r(b), r(q));
    let uu = dot(&up, &up);
    // The dot product of two vectors' shadows.
    let flat = |x: &R3, y: &R3| dot(x, y) - dot(&up, x) * dot(&up, y) / &uu;
    let (d, w) = (sub(&rb, &ra), sub(&rq, &ra));
    let dd = flat(&d, &d);
    let t = if dd > Rational::ZERO {
        (flat(&w, &d) / dd).clamp(Rational::ZERO, Rational::ONE)
    } else {
        Rational::ZERO
    };
    let foot: R3 = std::array::from_fn(|k| &ra[k] + &d[k] * &t);
    let want = dot(&up, &sub(&foot, &rq)).sign();
    let near = |c: &Rational| f64::rounding_from(c, Nearest).0;
    let mut x = DVec3::new(near(&foot[0]), near(&foot[1]), near(&foot[2]));
    if want != Ordering::Equal {
        // UP's z is its largest component: a step in z moves the height
        // most.
        for _ in 0..64 {
            if dot(&up, &sub(&r(x), &rq)).sign() == want {
                break;
            }
            x.z = if want == Ordering::Greater {
                x.z.next_up()
            } else {
                x.z.next_down()
            };
        }
    }
    x
}

/// A plane face's plane `n·x = c`, its normal outward; `None` for a
/// curved face.
fn plane_of(b: &crate::topo::brep_min::Brep, f: FaceId) -> Option<(DVec3, f64)> {
    let crate::geom::form::FormKey::Plane(k) = b.surface(f).lattice().key() else {
        return None;
    };
    let (n, c) = (DVec3::from_array(k.normal()), k.offset());
    Some(if b.flipped(f) { (-n, -c) } else { (n, c) })
}

/// The counting's answers for a placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Answers {
    /// Per operand, its vertices' windings in the other, carried from
    /// the seeds.
    pub winding: [Vec<i64>; 2],
    /// Per operand, its vertices' own columns: `Σ_f s02` (`s20`).
    pub column: [Vec<i64>; 2],
    /// `x12[e][f]`, `e` of `A`, `f` of `B`.
    pub x12: Vec<Vec<i64>>,
    /// `x21[g][f]`, `g` of `B`, `f` of `A`.
    pub x21: Vec<Vec<i64>>,
    /// Lobe fallbacks the windings took, both operands.
    pub lobes: u64,
}

/// One operand's vertices counted against the other's faces
/// ([`World::face_counts`]): per vertex and face its count and pieces, and
/// the height records they read.
struct FaceCounts {
    counts: Vec<Vec<i64>>,
    pieces: Vec<Vec<FacePieces>>,
    heights: Heights,
}

/// The records of a placement.
struct Recs {
    shadows: Shadows,
    curved: CurvedRays,
    feet: NearFeet,
}

fn refused(cause: Cause, at: impl Into<String>) -> Refusal {
    Refusal {
        cause,
        at: at.into(),
    }
}

fn record_fail<E: std::fmt::Debug>(kind: &'static str, e: E) -> Refusal {
    refused(
        Cause::Record {
            kind,
            fail: format!("{e:?}"),
        },
        kind,
    )
}

impl World<'_> {
    fn records(&self, cfg: &Config, stats: &mut Stats) -> Result<Recs, Refusal> {
        let band = Band::curved(0.0, cfg.target).expect("a band");
        let (mut flat, mut curved, mut vv) = (Vec::new(), Vec::new(), Vec::new());
        for op in [A, B] {
            let (s, o) = (self.solid(op), self.solid(op.other()));
            for v in s.verts() {
                let q = Point::vert(op, v);
                for g in o.edges() {
                    let k = RayKey { q, g };
                    if self.straight(op.other(), g) {
                        flat.push(k);
                    } else {
                        curved.push(k);
                    }
                }
                vv.extend(o.verts().into_iter().map(|w| VvKey::of(q, w).key));
            }
        }
        let mut shadows = Shadows::new();
        shadows
            .fill_vv(vv, self, &mut Tally { kind: "vv", stats })
            .map_err(|e| record_fail("vv", e))?;
        shadows
            .fill_rays(flat.clone(), self, &mut Tally { kind: "ray", stats })
            .map_err(|e| record_fail("ray", e))?;
        let mut rays = CurvedRays::new();
        rays.fill_chains(
            curved.clone(),
            &mut shadows,
            self,
            band,
            &mut Tally {
                kind: "curved ray",
                stats,
            },
        )
        .map_err(|e| record_fail("curved ray", e))?;
        let mut feet = NearFeet::new();
        feet.fill(
            flat.into_iter().chain(curved),
            self,
            &mut Tally {
                kind: "feet",
                stats,
            },
        )
        .map_err(|e| record_fail("feet", e))?;
        Ok(Recs {
            shadows,
            curved: rays,
            feet,
        })
    }

    /// Point `q`'s ray record against edge `g` of the other operand as a
    /// chain.
    fn chain(&self, r: &Recs, q: Point, g: EdgeId) -> ChainRec {
        let op = q.operand.other();
        if self.straight(op, g) {
            let rec = r.shadows.ray(q, g).expect("decided");
            let [a, b] = self.edge_points(op, g);
            let at = self.solid(q.operand).brep.pos(vert(q));
            line_chain(&rec, a, b, at)
        } else {
            r.curved.chain(q, g).expect("decided").clone()
        }
    }

    /// `s02(v, f)` (`s20`) for every vertex `v` of `op` and face `f` of
    /// the other, by vertex then face, with the pieces and the height
    /// records they read.
    fn face_counts(
        &self,
        r: &Recs,
        op: Operand,
        cfg: &Config,
        stats: &mut Stats,
    ) -> Result<FaceCounts, Refusal> {
        let (s, o) = (self.solid(op), self.solid(op.other()));
        let reads = Records {
            shadows: &r.shadows,
            feet: &r.feet,
            order: &self.order,
        };
        let mut sides = Sides::new();
        let mut heights = Heights::new();
        let band = Band::curved(0.0, cfg.target).expect("a band");
        let charge = Charge::new(u64::MAX / 4);
        let verts = s.verts();
        let mut out = vec![Vec::new(); verts.len()];
        let mut kept = vec![Vec::new(); verts.len()];
        for f in o.faces() {
            let face = FaceSetup::new(self, op.other(), f)?;
            let views = face.views();
            let uv = face.uv(op.other(), f, &views, cfg.scale)?;
            let t = std::time::Instant::now();
            let corners = Corners::new(&uv, &face.patches, &face.order)
                .map_err(|e| refused(Cause::Other(format!("corners {e:?}")), format!("{f:?}")))?;
            stats.corners += t.elapsed().as_secs_f64();
            let nets = self.flats[ix(op.other())][f.index()]
                .as_ref()
                .map(|d| d.nets.as_slice());
            // The face's rates less the vertices' (`A` moves along `d`).
            let motion: Motion = {
                let (fr, qr) = (self.rates(op.other()), self.rates(op));
                std::array::from_fn(|n| {
                    let (r, d) = (rate_rational(&fr[n]), rate_rational(&qr[n]));
                    std::array::from_fn(|c| &r[c] - &d[c])
                })
            };
            for &v in &verts {
                let q = Point::vert(op, v);
                let at = s.brep.pos(v);
                let recs: Vec<ChainRec> = face
                    .trims
                    .iter()
                    .map(|t| self.chain(r, q, t.edge))
                    .collect();
                let edges: Vec<TrimEdge<'_>> = face
                    .trims
                    .iter()
                    .zip(&recs)
                    .map(|(t, rays)| TrimEdge {
                        edge: t.edge,
                        ribbon: Ribbon {
                            coedge: t.coedge,
                            curve: o.brep.edge_spans(t.edge),
                            map: &t.map,
                            rays,
                        },
                    })
                    .collect();
                let fq = FaceQuery {
                    face: &uv,
                    corners: &corners,
                    patches: &face.patches,
                    edges: &edges,
                    point: q,
                    at,
                    eps_tie: cfg.eps_tie,
                    nets,
                    motion: &motion,
                };
                let here = || format!("{q:?} face {f:?}");
                // Side and dyadic point records asked for are filled, one
                // at a time, and the count run again.
                let mut asked = 0;
                let (res, cost) = loop {
                    let (res, cost) = count_pieces(&fq, &sides, &reads, &charge);
                    match res {
                        Err(CountError::Need(k)) if asked < 256 => {
                            asked += 1;
                            stats.side_fills += 1;
                            sides
                                .fill(
                                    [k],
                                    self,
                                    band,
                                    &mut Tally {
                                        kind: "side",
                                        stats,
                                    },
                                )
                                .map_err(|e| refused(Cause::Side(format!("{e:?}")), here()))?;
                        }
                        res => break (res, cost),
                    }
                };
                stats.counts[top_level(&cost)] += 1;
                let p = res.map_err(|e| refused(count_cause(e), here()))?;
                let n = self.recount(&reads, &p, &[], &mut heights, band, stats)?;
                out[v.index()].push(n);
                kept[v.index()].push(p);
            }
        }
        Ok(FaceCounts {
            counts: out,
            pieces: kept,
            heights,
        })
    }

    /// The count of `p`, its hits near the edges `lobe` by the edge rule,
    /// the height records it reads filled first.
    fn recount(
        &self,
        reads: &Records<'_, Order>,
        p: &FacePieces,
        lobe: &[EdgeId],
        heights: &mut Heights,
        band: Band,
        stats: &mut Stats,
    ) -> Result<i64, Refusal> {
        let here = || format!("{:?} face {:?}", p.point, p.face);
        let wanted = p
            .wanted(reads, lobe)
            .map_err(|e| refused(count_cause(e), here()))?;
        if !wanted.is_empty() {
            stats.height_fills += 1;
            heights
                .fill(
                    wanted,
                    &ModelHeights { model: self },
                    band,
                    &mut Tally {
                        kind: "height",
                        stats,
                    },
                )
                .map_err(|e| {
                    let cause = match e {
                        HeightError::Pass(Fail::Escalate { .. }) => Cause::Height,
                        e => Cause::Count(format!("heights {e:?}")),
                    };
                    refused(cause, here())
                })?;
        }
        p.count(reads, heights, lobe)
            .map_err(|e| refused(count_cause(e), here()))
    }

    /// `s11(e, g)` for every edge pair, `[e][g]`.
    fn s11s(&self, r: &Recs, cfg: &Config, stats: &mut Stats) -> Result<Vec<Vec<i64>>, Refusal> {
        let band = Band::curved(0.0, cfg.target).expect("a band");
        let base = XBase {
            shadows: &r.shadows,
            curved: &r.curved,
        };
        let mut sides = XSides::new();
        let mut heights = Heights::new();
        let (a, b) = (self.solid(A), self.solid(B));
        let curve = |op: Operand, e: EdgeId| {
            if self.straight(op, e) {
                let [p, q] = self.edge_points(op, e);
                XCurve::Line(p, q)
            } else {
                XCurve::Spans(self.solid(op).brep.edge_spans(e))
            }
        };
        let rays = |q: Point, g: EdgeId| {
            if self.straight(q.operand.other(), g) {
                EdgeRays::Line(r.shadows.ray(q, g).expect("decided"))
            } else {
                EdgeRays::Chain(r.curved.chain(q, g).expect("decided"))
            }
        };
        let mut out = Vec::new();
        let mut first = None;
        for e in a.edges() {
            let mut row = Vec::new();
            for g in b.edges() {
                let [p0, p1] = a.brep.ends(e).map(|v| Point::vert(A, v));
                let [g0, g1] = b.brep.ends(g).map(|v| Point::vert(B, v));
                let pair = XPair {
                    e: curve(A, e),
                    g: curve(B, g),
                    terms: XTerms {
                        a_g: rays(p0, g),
                        b_g: rays(p1, g),
                        g0_e: rays(g0, e),
                        g1_e: rays(g1, e),
                    },
                    shared: false,
                };
                let x = XKey { e, g };
                let mut one = || -> Result<i64, Refusal> {
                    let (se, sg) = (
                        self.solid(A).brep.edge_spans(e),
                        self.solid(B).brep.edge_spans(g),
                    );
                    // One curve: bitwise the same spans, either way round.
                    let shared = se == sg
                        || (se.len() == sg.len()
                            && se
                                .iter()
                                .rev()
                                .zip(sg)
                                .all(|(a, b)| [a[2], a[1], a[0]] == *b));
                    if shared {
                        stats.shared += 1;
                        let pair = XPair {
                            shared: true,
                            ..pair
                        };
                        let reads = Records {
                            shadows: &r.shadows,
                            feet: &r.feet,
                            order: &self.order,
                        };
                        let rec =
                            varde_decide::records::xrec::shared_clusters(&reads, self, x, &pair)
                                .map_err(|err| {
                                    let cause = match err {
                                        varde_decide::records::xrec::SharedError::Crossed => {
                                            Cause::Crossed
                                        }
                                        err => Cause::Clusters(format!("shared {err:?}")),
                                    };
                                    refused(cause, format!("{e:?} {g:?}"))
                                })?;
                        return self.s11_of(x, &rec, &mut heights, band, stats);
                    }
                    // The side and corner records the boxes ask for are
                    // filled, one at a time, and the clusters formed again.
                    let mut asked = 0;
                    let c = loop {
                        match form(&self.order, x, &pair, &sides) {
                            Err(XClusterError::Need(ks)) if asked < 16 => {
                                asked += 1;
                                for k in ks {
                                    stats.x_side_fills += 1;
                                    sides
                                        .fill(
                                            [k],
                                            base,
                                            self,
                                            band,
                                            &mut Tally {
                                                kind: "x side",
                                                stats,
                                            },
                                        )
                                        .map_err(|err| {
                                            refused(
                                                Cause::Side(format!("{err:?}")),
                                                format!("{e:?} {g:?}"),
                                            )
                                        })?;
                                }
                            }
                            r => break r,
                        }
                    }
                    .map_err(|err| {
                        let cause = match err {
                            XClusterError::Collinear => Cause::ZOrder,
                            err => Cause::Clusters(format!("{err:?}")),
                        };
                        refused(cause, format!("{e:?} {g:?}"))
                    })?;
                    stats.clusters[c.rec.level] += 1;
                    stats.clear +=
                        c.rec.clusters.iter().filter(|c| c.clear.is_some()).count() as u64;
                    self.s11_of(x, &c.rec, &mut heights, band, stats)
                };
                match one() {
                    Ok(s) => row.push(s),
                    Err(r) => {
                        *stats.pair_causes.entry(cause_key(&r.cause)).or_default() += 1;
                        first.get_or_insert(r);
                        row.push(0);
                    }
                }
            }
            out.push(row);
        }
        match first {
            Some(r) => Err(r),
            None => Ok(out),
        }
    }

    /// `s11` of pair `x` from its crossing record `rec`: each cluster's
    /// z-order clear or read from its height record, decided on the
    /// record's clusters ([`XHeights`]).
    fn s11_of(
        &self,
        x: XKey,
        rec: &XRec,
        heights: &mut Heights,
        band: Band,
        stats: &mut Stats,
    ) -> Result<i64, Refusal> {
        let at = [XAt { x, rec }];
        let geom = XHeights {
            model: self,
            recs: &at,
            near: band.tau_sign(),
        };
        let here = || format!("{:?} {:?}", x.e, x.g);
        rec.s11(|ci, _| {
            let z = zorder_key(x, ci);
            stats.zorder_records += 1;
            heights
                .fill(
                    [z.height_key(x.g)],
                    &geom,
                    band,
                    &mut Tally {
                        kind: "z-order",
                        stats,
                    },
                )
                .map_err(|err| refused(Cause::ZOrder, format!("{} {ci:?}: {err:?}", here())))?;
            let r = heights
                .get(z.key, ZKey::anchor(x.g))
                .ok_or_else(|| refused(Cause::ZOrder, here()))?;
            Ok(z.above(&r))
        })
    }

    /// The faces of `op` with a coedge on edge `g`.
    fn faces_of(&self, op: Operand, g: EdgeId) -> Vec<FaceId> {
        let b = &self.solid(op).brep;
        self.solid(op)
            .faces()
            .into_iter()
            .filter(|&f| {
                b.loops(f)
                    .any(|l| b.loop_coedges(l).any(|c| b.edge(c) == g))
            })
            .collect()
    }

    /// `x12[e][f]` (`op` = `A`) or `x21[g][f]` (`op` = `B`) from the face
    /// counts of `op`'s vertices and `s11`.
    fn cross_counts(
        &self,
        op: Operand,
        counts: &[Vec<i64>],
        s11: &[Vec<i64>],
    ) -> Result<Vec<Vec<i64>>, Refusal> {
        let (s, o) = (self.solid(op), self.solid(op.other()));
        let boundary = |f: FaceId| -> Vec<Coedge> {
            o.brep
                .loops(f)
                .flat_map(|l| o.brep.loop_coedges(l))
                .map(|c| Coedge {
                    edge: o.brep.edge(c),
                    forward: !o.brep.reversed(c),
                })
                .collect()
        };
        let bad = |e| refused(Cause::Other(format!("cross {e:?}")), "x");
        let mut out = Vec::new();
        for e in s.edges() {
            let [p, q] = s.brep.ends(e);
            let mut row = Vec::new();
            for f in o.faces() {
                let ends = [counts[p.index()][f.index()], counts[q.index()][f.index()]];
                let x = match op {
                    A => x12(ends, &boundary(f), |g| {
                        Ok::<_, ()>(s11[e.index()][g.index()])
                    }),
                    B => x21(ends, &boundary(f), |a| {
                        Ok::<_, ()>(s11[a.index()][e.index()])
                    }),
                }
                .map_err(bad)?;
                row.push(x);
            }
            out.push(row);
        }
        Ok(out)
    }

    /// The windings of `op`'s vertices: seeded at its first cleared
    /// vertex, carried by the crossing counts' rows summed, with the lobe
    /// fallback at a winding outside `{0, 1}` (the vertex's hits near an
    /// edge within whose near radius it lies re-read by the edge rule).
    /// With the crossing counts as last counted and the fallbacks taken.
    fn windings_of(
        &self,
        op: Operand,
        r: &Recs,
        fc: &mut FaceCounts,
        s11: &[Vec<i64>],
        cfg: &Config,
        stats: &mut Stats,
    ) -> Result<Windings, Refusal> {
        let (s, o) = (self.solid(op), self.solid(op.other()));
        let hulls: Vec<HullBox> = o
            .faces()
            .into_iter()
            .flat_map(|f| {
                let surf = o.brep.surface(f);
                let r = surf.rect().clone();
                r.u.clone()
                    .flat_map(move |i| r.v.clone().map(move |j| (i, j)))
                    .map(|(i, j)| surf.tile(i, j).expect("a tile").hull_box())
                    .collect::<Vec<_>>()
            })
            .collect();
        let ends: Vec<[VertId; 2]> = s.edges().into_iter().map(|e| s.brep.ends(e)).collect();
        let graph = Graph {
            operand: op,
            verts: s.brep.vert_count() as u32,
            edges: &ends,
        };
        let (sr, _) = seeds(&self.order, &graph, 1 << 20, |v| {
            (Ok::<_, ()>(clear_of(s.brep.pos(v), cfg.eps_tie, &hulls)), 1)
        });
        let sd = sr.map_err(|e| refused(Cause::Seeds(format!("{e:?}")), format!("{op:?}")))?;
        for seed in &sd {
            if seed.cleared {
                stats.cleared_seeds += 1;
            } else {
                stats.uncleared_seeds += 1;
            }
        }
        // Per vertex, the edges of the other operand within whose near
        // radius it lies and near which one of its pieces holds a hit, in
        // geometric order.
        let faces_at: Vec<Vec<FaceId>> = o
            .edges()
            .into_iter()
            .map(|g| self.faces_of(op.other(), g))
            .collect();
        let zones: Vec<Vec<EdgeId>> = s
            .verts()
            .into_iter()
            .map(|v| {
                let q = Point::vert(op, v);
                let mut es: Vec<_> = o
                    .edges()
                    .into_iter()
                    .filter(|&g| {
                        r.feet.get(q, g).is_some_and(|ft| !ft.feet.is_empty())
                            && faces_at[g.index()]
                                .iter()
                                .any(|f| fc.pieces[v.index()][f.index()].near_edge(g))
                    })
                    .map(|g| (self.order.edge_key(op.other(), g), g))
                    .collect();
                es.sort();
                es.into_iter().map(|(_, g)| g).collect()
            })
            .collect();
        let reads = Records {
            shadows: &r.shadows,
            feet: &r.feet,
            order: &self.order,
        };
        let band = Band::curved(0.0, cfg.target).expect("a band");
        let mut lobes = Lobes::new();
        let w = windings_lobed(
            &self.order,
            &graph,
            &mut lobes,
            |lobes| {
                for q in lobes.points() {
                    let v = vert(q);
                    let at = lobes.at(q);
                    for f in o.faces() {
                        if !at.iter().any(|g| faces_at[g.index()].contains(&f)) {
                            continue;
                        }
                        let p = &fc.pieces[v.index()][f.index()];
                        let n = self.recount(&reads, p, &at, &mut fc.heights, band, stats)?;
                        fc.counts[v.index()][f.index()] = n;
                    }
                }
                let x = self.cross_counts(op, &fc.counts, s11)?;
                let seeded = sd
                    .iter()
                    .map(|seed| (seed.vert, fc.counts[seed.vert.index()].iter().sum()))
                    .collect();
                let dw = x.iter().map(|row| row.iter().sum()).collect();
                Ok((seeded, dw))
            },
            |q| Ok(zones[vert(q).index()].clone()),
        )
        .map_err(|e| match e {
            WindingError::Recount(r) => r,
            e => {
                let cause = if e.is_signal() {
                    Cause::Signal(match e {
                        WindingError::Disagree { .. } => "Disagree".into(),
                        _ => "Outside".into(),
                    })
                } else {
                    Cause::Other(format!("{e:?}"))
                };
                refused(cause, format!("{op:?}: {e:?}"))
            }
        })?;
        let x = self.cross_counts(op, &fc.counts, s11)?;
        Ok((w, x, lobes.count() as u64))
    }

    /// The counting's answers, or why it has none.
    pub(super) fn count(&self, cfg: &Config, stats: &mut Stats) -> Result<Answers, Refusal> {
        let t = std::time::Instant::now();
        let r = self.records(cfg, stats)?;
        stats.time[0] += t.elapsed().as_secs_f64();
        let t = std::time::Instant::now();
        let s11 = self.s11s(&r, cfg, stats);
        stats.time[2] += t.elapsed().as_secs_f64();
        let t = std::time::Instant::now();
        let mut fa = self.face_counts(&r, A, cfg, stats)?;
        let mut fb = self.face_counts(&r, B, cfg, stats)?;
        stats.time[1] += t.elapsed().as_secs_f64();
        let s11 = s11?;
        let (wa, x12s, la) = self.windings_of(A, &r, &mut fa, &s11, cfg, stats)?;
        let (wb, x21s, lb) = self.windings_of(B, &r, &mut fb, &s11, cfg, stats)?;
        stats.lobes += la + lb;
        let column = |c: &[Vec<i64>]| c.iter().map(|r| r.iter().sum()).collect();
        Ok(Answers {
            winding: [wa, wb],
            column: [column(&fa.counts), column(&fb.counts)],
            x12: x12s,
            x21: x21s,
            lobes: la + lb,
        })
    }
}

/// The refusal cause of a count error.
fn count_cause(e: CountError) -> Cause {
    match e {
        CountError::Need(_) => Cause::Side("asked again".into()),
        CountError::NeedHeight(_) | CountError::HitsUnformed { .. } => Cause::Height,
        CountError::OutOfBudget { cause, .. } => Cause::OutOfBudget(format!("{cause:?}")),
        e => Cause::Count(short(&e)),
    }
}

/// A short name for a count error.
fn short(e: &CountError) -> String {
    match e {
        CountError::Box(b) => format!("Box({})", box_kind(b)),
        CountError::Piece(p) => format!("Piece({p:?})"),
        CountError::Hits(h) => {
            let s = format!("{h:?}");
            let k = s.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
            format!("Hits({k})")
        }
        e => {
            let s = format!("{e:?}");
            s.split(|c: char| !c.is_alphanumeric())
                .next()
                .unwrap_or("")
                .to_string()
        }
    }
}

fn box_kind(b: &BoxError) -> String {
    let s = format!("{b:?}");
    s.split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("")
        .to_string()
}

/// One trim of a face: its coedge, edge, p-curve spans and their map
/// onto the edge's spans.
struct TrimSetup {
    coedge: CoedgeId,
    edge: EdgeId,
    reversed: bool,
    ends: [VertId; 2],
    band: f64,
    map: Vec<SpanMap>,
}

/// A face as the counting reads it.
struct FaceSetup<'s> {
    solid: &'s Solid,
    trims: Vec<TrimSetup>,
    patches: Vec<HomPatch<Bounded<f64>>>,
    /// Its vertices in geometric order.
    order: Vec<VertId>,
}

impl<'s> FaceSetup<'s> {
    fn new(w: &World<'s>, op: Operand, f: FaceId) -> Result<FaceSetup<'s>, Refusal> {
        let s = w.solid(op);
        let b = &s.brep;
        let mut trims = Vec::new();
        for l in b.loops(f) {
            for c in b.loop_coedges(l) {
                let e = b.edge(c);
                let map = span_map(b.pcurve(c).knots(), b.curve(e).knots(), b.t_range(e))
                    .ok_or_else(|| {
                        refused(
                            Cause::Other("p-curve knots off the edge's spans".into()),
                            format!("{c:?}"),
                        )
                    })?;
                trims.push(TrimSetup {
                    coedge: c,
                    edge: e,
                    reversed: b.reversed(c),
                    ends: b.ends(e),
                    band: w.band(op, e),
                    map,
                });
            }
        }
        let surf = b.surface(f);
        let r = surf.rect();
        let mut patches = Vec::new();
        for i in r.u.clone() {
            for j in r.v.clone() {
                patches.push(surf.tile(i, j).expect("a tile").patch().clone());
            }
        }
        let mut vs: Vec<VertId> = trims.iter().flat_map(|t| t.ends).collect();
        vs.sort_unstable();
        vs.dedup();
        let mut keyed: Vec<_> = vs
            .into_iter()
            .map(|v| (w.order.vert_key(op, v), v))
            .collect();
        keyed.sort();
        Ok(FaceSetup {
            solid: s,
            trims,
            patches,
            order: keyed.into_iter().map(|(_, v)| v).collect(),
        })
    }

    /// The trims as the face's UV domain reads them, borrowing the
    /// p-curves' spans from the store.
    fn views(&self) -> Vec<Trim<'s>> {
        let b = &self.solid.brep;
        self.trims
            .iter()
            .map(|t| Trim {
                coedge: t.coedge,
                reversed: t.reversed,
                spans: b.pcurve(t.coedge).spans(),
                ends: t.ends,
                zero_length: false,
                band: t.band,
            })
            .collect()
    }

    /// The face's UV domain over `views` ([`FaceSetup::views`]).
    fn uv<'t>(
        &self,
        op: Operand,
        f: FaceId,
        views: &'t [Trim<'t>],
        scale: [f64; 2],
    ) -> Result<FaceUv<'t>, Refusal> {
        let b = &self.solid.brep;
        let r = b.surface(f).rect();
        FaceUv::new(
            op,
            f,
            [r.u.clone(), r.v.clone()],
            {
                let u = self.solid.scale[f.index()];
                [u[0] * scale[0], u[1] * scale[1]]
            },
            views,
            b.flipped(f),
        )
        .map_err(|e| refused(Cause::Other(format!("face {e:?}")), format!("{f:?}")))
    }
}

/// A cause's name without its keys.
pub(super) fn cause_key(c: &Cause) -> String {
    let s = format!("{c:?}");
    s.split(['{', '(']).next().unwrap_or("").to_string()
}

/// Where each p-curve span lies on the edge's spans (from the edge's
/// first span inside `t_range`): the edge span holding its knots' range,
/// and that range on the span's `[0, 1]`, linearly; `None` if a p-curve
/// span straddles an edge knot.
fn span_map(pk: &[f64], ek: &[f64], t_range: [f64; 2]) -> Option<Vec<SpanMap>> {
    let first = ek.iter().position(|&k| k == t_range[0])?;
    let mut out = Vec::new();
    for w in pk.windows(2) {
        let (a, b) = (w[0], w[1]);
        let k = (first..ek.len() - 1).find(|&k| ek[k] <= a && b <= ek[k + 1])?;
        let (t0, t1) = (ek[k], ek[k + 1]);
        let at = |t: f64| {
            if t == t0 {
                0.0
            } else if t == t1 {
                1.0
            } else {
                (t - t0) / (t1 - t0)
            }
        };
        out.push(SpanMap {
            span: (k - first) as u32,
            t: [at(a), at(b)],
        });
    }
    Some(out)
}
