//! Edge-in-surface records: whether an existing edge `g` lies in a
//! surface `S` on the side of one of its faces `f`, decided exactly on
//! the stored data, **per coedge** (the use `c` of `g` by `f`), never by
//! distance and whatever `g`'s curve came from (constructed, fitted, or
//! refit after its vertex moved). A boolean asks it where an arc on `f`
//! comes from `f`'s surface and a surface `S` of the other operand: when
//! it holds, the arc is a piece of `g` on `f` (a cut through an existing
//! edge or seam), not an arc beside it ([`t0_coincident`]).
//!
//! **Clauses** ([`Clause`]), tried in this order, the first that holds
//! the answer:
//! - **same form:** `f`'s surface has `S`'s form key, bit for bit, so
//!   all of `f` lies in `S`;
//! - **the curve** (`S` a plane `n·x = d`): every affine control point of
//!   `g`'s curve over the edge's range satisfies `n·x = d` exactly
//!   ([`PlaneSide`]). A span with positive weights is a weighted mean of
//!   its control points, so it then lies in `S`; and as a homogeneous
//!   point `(w·x, w)` meets `n·X = d·W` exactly when `x` meets `n·x = d`
//!   (`w > 0`), the test on the affine points is the test on the
//!   homogeneous ones;
//! - **the face-side trace** (`S` a plane): every control point of `f`'s
//!   surface along `c`'s p-curve, `S_f∘p_c`, satisfies `n·x = d` exactly.
//!   On a tile that is an exact affine map (below) the trace of a span
//!   with one weight for both coordinates (Q and linear spans; an iso
//!   span, whose fixed coordinate has weight 1) is the span of the images
//!   of its control points, `o + s·(a − o) + t·(b − o)` at each point's
//!   local `(s, t)`, with its weights: each image is tested as it stands,
//!   never formed as a point ([`ParallelogramSide`]). Either curve
//!   suffices: the trace and `g`'s curve lie within the coedge's seam
//!   bound of each other, so a piece reusing `g` stays within the reuse
//!   bound. The trace holds where `g`'s curve was refit off `S` (its end
//!   vertex moved) while `f` kept its p-curve.
//!
//! Otherwise the record is false: a surface `S` not a plane (the
//! revolution forms' rulings and circles are a later clause), a trace on
//! a tile that isn't an exact affine map (a curved face, or a plane whose
//! rounded net isn't a parallelogram; the exact composition with the
//! tile is a later clause), or a graph span, whose coordinates have
//! weights of their own (no plane face's p-curve has one). A false
//! answer where `g` truly lies in `S` is no sliver: the boolean then
//! makes an offset arc beside `g`, and its check refuses the two faces
//! touching without adjacency.
//!
//! **Affine tiles.** A tile is an exact affine map when every weight of
//! its net is 1 and every control point is the affine combination of the
//! corners `o = P₀₀`, `a = P₂₀`, `b = P₀₂` at its grid place,
//! `2·Pᵢⱼ = (2 − i − j)·o + i·a + j·b` exactly ([`AffineGap`]): the
//! biquadratic patch is then `o + s·(a − o) + t·(b − o)` (Bernstein
//! polynomials reproduce linear functions). A plane's tile whose frame
//! and cell rounded exactly (an axis-aligned plane, dyadic offsets) is
//! one; an oblique plane's rounded net is in general not.
//!
//! **Inputs** ([`Coedge`], [`Ask`]): the face's surface, the coedge's
//! p-curve and the edge's curve spans over its range, independent of how
//! the store lays them out ([`Coedge::of`] reads them from the minimal
//! store; the full store gives the same three).
//!
//! **Work.** One pass on the operation's budget, an item per ask: a unit,
//! then each exact sign at the record units' price of its kind, the
//! filter charged before it runs and any later stage after. An ask stops
//! at the first clause that holds and, within a clause, at the first
//! control point off `S`. The answers are the caller's records: decided
//! once per `(coedge, surface)` and read by every consumer.

use std::cmp::Ordering;

use glam::DVec3;
use varde_decide::bezier::Dir;
use varde_decide::num::exact::{Num, Pred, value_sign};
use varde_decide::num::preds::{ParallelogramSide, PlaneSide};
use varde_decide::num::schedule::{PredKind, UNIT_PARTS, UNITS};

use crate::geom::certify::tile_of;
use crate::geom::curve::Span3;
use crate::geom::form::{FormKey, PlaneKey};
use crate::geom::pcurve::{PCurve, PSpan};
use crate::geom::surface::Surface;
use crate::geom::tile::{Net, Tile};
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;
use crate::topo::brep_min::{Brep, CoedgeId};

/// The clause an edge-in-surface record holds by (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clause {
    /// The face's surface has the surface's form key.
    SameForm,
    /// Every control point of the edge's curve lies in the plane.
    Curve,
    /// Every control point of the face-side trace lies in the plane.
    Trace,
}

/// A coedge's geometry as the record reads it (module docs).
#[derive(Clone, Copy, Debug)]
pub struct Coedge<'a> {
    /// Its face's surface.
    pub surface: &'a Surface,
    /// Its p-curve on that surface.
    pub pcurve: &'a PCurve,
    /// Its edge's curve spans over the edge's range.
    pub curve: &'a [Span3],
}

impl<'a> Coedge<'a> {
    /// The coedge `c` of `brep`: its face's surface, its p-curve and its
    /// edge's spans ([`Brep::edge_spans`]).
    pub fn of(brep: &'a Brep, c: CoedgeId) -> Coedge<'a> {
        Coedge {
            surface: brep.surface(brep.face(c)),
            pcurve: brep.pcurve(c),
            curve: brep.edge_spans(brep.edge(c)),
        }
    }
}

/// One record to decide: does the coedge's edge lie in the surface, on
/// its face's side?
#[derive(Clone, Copy, Debug)]
pub struct Ask<'a> {
    /// The coedge.
    pub coedge: Coedge<'a>,
    /// The surface, by its form key.
    pub surface: &'a FormKey,
}

/// Why an ask can't be decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The edge has no span over its range, or the p-curve none.
    Empty,
    /// This span of the p-curve lies in no tile of the surface.
    Domain {
        /// The span.
        span: usize,
    },
    /// A stored value isn't finite.
    NotFinite,
}

/// Why the records can't be decided.
#[derive(Clone, Debug, PartialEq)]
pub enum EdgeOnError {
    /// The ask at this position can't be decided.
    Ask {
        /// Its position.
        ask: usize,
        /// Why.
        fault: Fault,
    },
    /// The budget ran out, or a fixed limit was reached
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// Why an arc is coincident with an existing edge by the first-tier
/// rule ([`t0_coincident`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum T0 {
    /// The arc's other surface has the form key of the face across the
    /// edge: the arc's two surfaces are the edge's two faces'.
    Across,
    /// The edge lies in the arc's other surface on the arc's face, by
    /// this clause.
    EdgeOn(Clause),
}

/// The first-tier coincidence of an arc on a face `f` with the edge `g`
/// of one of `f`'s coedges: the arc comes from `f`'s surface and `other`,
/// `across` is the form key of the face across `g` from `f` (`None` on
/// an open boundary), `edge_on` the record of `g`'s coedge on `f` against
/// `other`. Coincident when `other` is `across`'s form (the arc's curve
/// is then `g`'s), else when the record holds; `None` otherwise. Reads
/// records decided elsewhere and decides nothing again. Shared curves and
/// pairs both of first tier are their own records.
pub fn t0_coincident(
    other: &FormKey,
    across: Option<&FormKey>,
    edge_on: Option<Clause>,
) -> Option<T0> {
    if across.is_some_and(|k| k == other) {
        return Some(T0::Across);
    }
    edge_on.map(T0::EdgeOn)
}

/// The records of `asks` (module docs), in their order, charged to
/// `work` in one pass: each the clause it holds by, `None` where it is
/// false.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the boolean's face arrangements call it as they land"
    )
)]
pub(crate) fn decide(
    asks: &[Ask<'_>],
    work: &mut Work,
) -> Result<Vec<Option<Clause>>, EdgeOnError> {
    work.pass(
        Pass {
            stage: "edge-on records",
            rung: 0,
        },
        asks,
        |ask, ledger| edge_on(ask, ledger),
    )
    .map_err(|halt| match halt {
        Halt::Failed { item, error } => EdgeOnError::Ask {
            ask: item,
            fault: error,
        },
        Halt::OutOfBudget(e) => EdgeOnError::OutOfBudget(e),
    })
}

/// The record of `ask` (module docs), charged to `ledger`: for a stage
/// deciding it inside a pass of its own.
pub(crate) fn edge_on(
    ask: &Ask<'_>,
    ledger: &mut Ledger<'_>,
) -> Result<Option<Clause>, Stop<Fault>> {
    let mut m = Meter { ledger, parts: 0 };
    m.ledger.charge(Step::Work, 1)?;
    let Coedge {
        surface,
        pcurve,
        curve,
    } = ask.coedge;
    if curve.is_empty() || pcurve.spans().is_empty() {
        return Err(Stop::Failed(Fault::Empty));
    }
    if surface.lattice().key() == ask.surface {
        return Ok(Some(Clause::SameForm));
    }
    let FormKey::Plane(plane) = ask.surface else {
        return Ok(None);
    };
    if curve_in(curve, plane, &mut m)? {
        return Ok(Some(Clause::Curve));
    }
    if trace_in(surface, pcurve, plane, &mut m)? {
        return Ok(Some(Clause::Trace));
    }
    Ok(None)
}

/// Whether every control point of `curve` lies in `plane`.
fn curve_in(curve: &[Span3], plane: &PlaneKey, m: &mut Meter<'_, '_>) -> Res<bool> {
    let normal = DVec3::from_array(plane.normal());
    for span in curve {
        for &[x, y, z, _] in span {
            let side = PlaneSide {
                normal,
                offset: plane.offset(),
                p: DVec3::new(x, y, z),
            };
            if m.sign(&side)? != Ordering::Equal {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Whether every control point of the trace of `pcurve` on `surface`
/// lies in `plane`, on tiles that are exact affine maps (module docs);
/// false where a span's trace can't be tested so.
fn trace_in(
    surface: &Surface,
    pcurve: &PCurve,
    plane: &PlaneKey,
    m: &mut Meter<'_, '_>,
) -> Res<bool> {
    let normal = DVec3::from_array(plane.normal());
    // The last tile found affine, by its indices: a plane face's spans
    // all lie in its one tile.
    let mut affine: Option<(i64, i64)> = None;
    for (k, span) in pcurve.spans().iter().enumerate() {
        let Some(uvs) = control_uvs(span) else {
            return Ok(false);
        };
        let (tile, i, j) = tile_of(surface, span).ok_or(Stop::Failed(Fault::Domain { span: k }))?;
        if affine != Some((i, j)) {
            if !is_affine(tile, m)? {
                return Ok(false);
            }
            affine = Some((i, j));
        }
        let corner = |a: usize, b: usize| DVec3::from_array(tile.point(a, b));
        let (o, a, b) = (corner(0, 0), corner(2, 0), corner(0, 2));
        for uv in uvs {
            let side = ParallelogramSide {
                normal,
                offset: plane.offset(),
                o,
                a,
                b,
                uv,
                // Tile indices below 2⁵² in magnitude (`tile_of`): exact.
                corner: [i as f64, j as f64],
            };
            if m.sign(&side)? != Ordering::Equal {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The UV control points of a span whose coordinates share their
/// weights (an iso span's fixed coordinate has weight 1): its trace on
/// an affine tile is the span of their images with those weights. `None`
/// for a graph span.
fn control_uvs(span: &PSpan) -> Option<Vec<[f64; 2]>> {
    match span {
        PSpan::Q(s) => Some(s.iter().map(|p| [p[0], p[1]]).collect()),
        PSpan::Linear(s) => Some(s.iter().map(|p| [p[0], p[1]]).collect()),
        PSpan::Iso { fixed, at, m } => Some(
            m.iter()
                .map(|p| match fixed {
                    Dir::U => [*at, p[0]],
                    Dir::V => [p[0], *at],
                })
                .collect(),
        ),
        PSpan::G { .. } => None,
    }
}

/// Whether `tile` is an exact affine map (module docs): weights 1 and
/// every control point at its grid place of the parallelogram of the
/// corners `P₀₀`, `P₂₀`, `P₀₂`.
fn is_affine(tile: &Tile, m: &mut Meter<'_, '_>) -> Res<bool> {
    let net: &Net = tile.net();
    if net.iter().any(|p| p[3] != 1.0) {
        return Ok(false);
    }
    let (o, a, b) = (net[0], net[6], net[2]);
    for (n, p) in net.iter().enumerate() {
        let (i, j) = (n / 3, n % 3);
        // The corners themselves hold by definition.
        if matches!((i, j), (0, 0) | (2, 0) | (0, 2)) {
            continue;
        }
        for k in 0..3 {
            let gap = AffineGap {
                p: p[k],
                o: o[k],
                a: a[k],
                b: b[k],
                at: [i as f64, j as f64],
            };
            if m.sign(&gap)? != Ordering::Equal {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// `2·p − (2 − i − j)·o − i·a − j·b` for one coordinate of the control
/// point `p` at grid place `(i, j)` of a biquadratic net and the corners
/// `o`, `a`, `b` at `(0, 0)`, `(2, 0)`, `(0, 2)`: zero where `p` is the
/// parallelogram's point there. The factors are small integers, exact;
/// a linear form, priced as [`PredKind::Side`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct AffineGap {
    p: f64,
    o: f64,
    a: f64,
    b: f64,
    at: [f64; 2],
}

impl Pred for AffineGap {
    fn eval<N: Num>(&self) -> N {
        let [i, j] = self.at;
        let lit = N::lit;
        lit(2.0)
            .mul(&lit(self.p))
            .sub(&lit(2.0 - i - j).mul(&lit(self.o)))
            .sub(&lit(i).mul(&lit(self.a)))
            .sub(&lit(j).mul(&lit(self.b)))
    }

    fn kind(&self) -> PredKind {
        PredKind::Side
    }
}

type Res<T> = Result<T, Stop<Fault>>;

/// An ask's ledger, with the parts of a unit not yet charged.
struct Meter<'l, 'p> {
    ledger: &'l mut Ledger<'p>,
    parts: u64,
}

impl Meter<'_, '_> {
    fn parts(&mut self, p: u64) -> Res<()> {
        self.parts = self.parts.saturating_add(p);
        let whole = self.parts / UNIT_PARTS;
        self.parts %= UNIT_PARTS;
        if whole > 0 {
            self.ledger.charge(Step::Work, whole)?;
        }
        Ok(())
    }

    /// The exact sign of `pred`: its filter charged before, any further
    /// stage after.
    fn sign(&mut self, pred: &impl Pred) -> Res<Ordering> {
        let filter = UNITS.record.of(pred.kind()).filter;
        self.parts(filter)?;
        let (s, w) = value_sign(pred);
        self.parts(w.parts(&UNITS.record).saturating_sub(filter))?;
        s.map_err(|_| Stop::Failed(Fault::NotFinite))
    }
}

#[cfg(test)]
mod tests;
