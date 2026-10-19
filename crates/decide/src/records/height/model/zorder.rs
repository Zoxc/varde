//! The z-order decider: the edge–edge keys `X(e, g, c)` and the
//! shared-curve keys `XRoot(e, g, c)` of a model's crossing records, at
//! their anchor `g` ([`XHeights`]).
//!
//! **The row a key reads** is the cluster's, fixed with it when the
//! crossing record was formed ([`ZRow`]):
//!
//! - **edge–edge**, two straight edges: the flat predicate on their
//!   segments ([`EdgeEdge`](crate::perturb::EdgeEdge), the model's
//!   [`HeightModel::edge_seg`]), exact;
//! - **edge–edge** with a curved edge: at a precision level, over the
//!   cluster's box. The cluster holds one crossing of the two shadows,
//!   its shadow tangents transversal over the box ([`crate::count::xrec`]
//!   forms no other unclear cluster), so the crossing is found by halving
//!   the larger piece of each sub-box and keeping the sub-boxes whose
//!   shadows' hulls aren't certified apart, until the gap in height
//!   between the two pieces is certified off 0 by more than its width,
//!   the pieces stop shrinking at the level, or the kept sub-boxes stop
//!   being few; then Krawczyk steps at the level from the hull of their
//!   parameters close in on it where they can. The **value** is the gap
//!   at the crossing, `ê·(C_g − C_e)`: a positive multiple (`1/(m·ê)`)
//!   of `m·(C_g − C_e)`, the shadow difference being 0 there, enclosed by
//!   the narrower of the two. The **orders** `m·(Δg − Δe)`
//!   are enclosed over the kept sub-boxes from the cones of their control
//!   point differences ([`z_order_cone`]), the rates the model's there;
//! - **shared-curve z-order** of a root cluster (`XRoot(e, g, c)`, a sign
//!   change of `h` along the overlap of two edges on one curve,
//!   [`ZRow::Root`]): the value exactly 0 (the heights coincide there),
//!   the orders the shared-curve row ([`shared_zorder`]) at the level over
//!   the cluster's piece of `e`, from the curve's tangent and `e`'s rates
//!   there; decided only where order 1 is certified one sign over the
//!   piece (the root clusters are formed only so), else it escalates.
//!
//! **Near ties.** A curved edge–edge value is exactly 0 only where an end
//! of one edge in the cluster's box lies exactly on the other edge's
//! piece there (decided on the stored data): then the orders decide, as
//! at a straight shared corner. Any other value is decided by its sign,
//! never a tie (the column hits and corners it must agree with are
//! exact, so a band tie here would mix a perturbed z-order with exact
//! neighbours), and only where certified farther than the curved
//! records' target `near` from 0: a value within it is a near tie that
//! other records (a curved ray record's band tie at a near end) may have
//! taken as a tie, so it escalates, and past the last level the count is
//! refused, never answered from a mix.
//!
//! **Shared corners.** Where an end of each edge in the cluster's box is
//! the same point (a vertex on a vertex) and one edge is straight, that
//! end lies on the other edge's piece too: the value is exactly 0 and the
//! orders decide, as at any end on the other edge. The face counts at
//! that vertex agree with it: a face's hits within `ε_tie` of the vertex
//! (a tile missing it by its rounding beside the ribbon through it) read
//! the vertex's germ key there with the ribbon's hit
//! ([`crate::count::s02`]). Two curves from one vertex (arcs on one
//! circle, stacked) aren't decided here: refused, never answered
//! unchecked.
//!
//! Every other key, and an edge–edge key whose pair has no record here,
//! is [`ModelHeights`]'.

use std::cmp::Ordering;

use glam::DVec3;
use malachite::Rational;

use super::{FlatHeight, HeightModel, ModelHeights, on_span_in};
use crate::bezier::{HomCurve, Scalar};
use crate::count::xrec::cut_part;
use crate::num::bounded::up;
use crate::num::exact::{ORDERS, rat};
use crate::perturb::{Curved, NU, R_HAT, UP, between, curve_point, shared_zorder, z_order_cone};
use crate::records::height::{AnchorRow, HeightGeometry, KeyValue};
use crate::records::keys::{Anchor, HKey, HeightKey, Operand, XKey};
use crate::records::shadow::LevelWork;
use crate::records::xrec::{XCluster, XCurve, XGeometry, XRec, ZRow};

/// An edge pair's crossing record as its z-order keys read it.
#[derive(Clone, Copy, Debug)]
pub struct XAt<'a> {
    /// The pair.
    pub x: XKey,
    /// Its record.
    pub rec: &'a XRec,
}

/// The height geometry of a model with the crossing records whose
/// edge–edge keys it decides (module docs).
#[derive(Clone, Copy, Debug)]
pub struct XHeights<'a, M> {
    /// The model.
    pub model: &'a M,
    /// The crossing records, any order.
    pub recs: &'a [XAt<'a>],
    /// The curved records' error target (a length): a curved edge–edge
    /// value within it of 0 but not exactly 0 isn't decided (module docs,
    /// "Near ties").
    pub near: f64,
}

/// A cluster as its key reads it.
struct ZAt<'a> {
    x: XKey,
    e: XCurve<'a>,
    g: XCurve<'a>,
    cl: XCluster,
}

impl<'a, M: HeightModel + XGeometry> XHeights<'a, M> {
    fn base(&self) -> ModelHeights<'a, M> {
        ModelHeights { model: self.model }
    }

    /// The cluster key `h` names, where its pair's record is here.
    fn cluster(&self, h: HKey) -> Option<ZAt<'a>> {
        let (HKey::X { e, g, c } | HKey::XRoot { e, g, c }) = h else {
            return None;
        };
        let x = XKey { e, g };
        let rec = self.recs.iter().find(|r| r.x == x)?.rec;
        Some(ZAt {
            x,
            e: self.model.x_curve(Operand::A, e),
            g: self.model.x_curve(Operand::B, g),
            cl: *rec.get(c)?,
        })
    }

    /// Whether an end of one edge in the cluster's box lies exactly on the
    /// other edge's piece there (module docs): the curved edge–edge
    /// value is then exactly 0.
    fn incident(z: &ZAt<'_>) -> bool {
        let ends = |c: &XCurve<'_>, part: &crate::records::xrec::XPart| -> Vec<[f64; 3]> {
            let last = c.span_count().saturating_sub(1);
            let mut out = Vec::new();
            if part.span == 0 && part.t[0] == 0.0 {
                out.extend(c.end(false));
            }
            if part.span == last && part.t[1] == 1.0 {
                out.extend(c.end(true));
            }
            out
        };
        let on = |c: &XCurve<'_>, part: &crate::records::xrec::XPart, p: [f64; 3]| match c {
            XCurve::Line(a, b) => on_segment(*a, *b, p, part.t),
            XCurve::Spans(s) => usize::try_from(part.span)
                .ok()
                .and_then(|k| s.get(k))
                .is_some_and(|sp| on_span_in(sp, p, part.t)),
        };
        ends(&z.e, &z.cl.e)
            .into_iter()
            .any(|p| on(&z.g, &z.cl.g, p))
            || ends(&z.g, &z.cl.g)
                .into_iter()
                .any(|p| on(&z.e, &z.cl.e, p))
    }

    /// Whether an end of each edge in the cluster's box is the same point,
    /// exactly, and both edges are curved: a vertex on a vertex of two
    /// curves (module docs, "Shared corners").
    fn curved_corner(z: &ZAt<'_>) -> bool {
        if !matches!((z.e, z.g), (XCurve::Spans(_), XCurve::Spans(_))) {
            return false;
        }
        let ends = |c: &XCurve<'_>, part: &crate::records::xrec::XPart| -> Vec<[f64; 3]> {
            let last = c.span_count().saturating_sub(1);
            let mut out = Vec::new();
            if part.span == 0 && part.t[0] == 0.0 {
                out.extend(c.end(false));
            }
            if part.span == last && part.t[1] == 1.0 {
                out.extend(c.end(true));
            }
            out
        };
        let g = ends(&z.g, &z.cl.g);
        ends(&z.e, &z.cl.e).iter().any(|p| g.contains(p))
    }

    /// Whether the cluster reads the flat edge–edge row: two straight
    /// edges.
    fn flat(z: &ZAt<'_>) -> bool {
        matches!((z.e, z.g), (XCurve::Line(..), XCurve::Line(..)))
    }

    /// The shared-curve z-order orders of a root cluster at `like`'s
    /// level over its piece of `e` (module docs); `None` where order 1
    /// isn't certified one sign there.
    fn root_orders<S: Curved>(&self, z: &ZAt<'_>, like: &S) -> Option<[S; ORDERS]> {
        let c = z.e.span(z.cl.e.span, like)?;
        let t = between(z.cl.e.t[0], z.cl.e.t[1], like);
        let (_, tangent) = curve_point(&c, &t);
        let rates = self.model.edge_rates(Operand::A, z.x.e, z.cl.e.span, &t);
        let ord = shared_zorder(&tangent, &rates)?;
        matches!(ord[0].sign(), Some(Ordering::Greater | Ordering::Less)).then_some(ord)
    }

    /// The kept sub-boxes of a curved cluster at `like`'s level and the
    /// gap `UP·(C_g − C_e)` over them (module docs); `None` where the
    /// pieces can't be formed.
    fn crossing<S: Curved>(
        &self,
        z: &ZAt<'_>,
        like: &S,
        work: &mut LevelWork,
    ) -> Option<(Vec<Sub<S>>, [f64; 2])> {
        let piece = |c: &XCurve<'_>, span: u32, t: [f64; 2]| -> Option<Piece<S>> {
            let full = c.span(span, like)?;
            Some(Piece {
                c: cut_part(&full, t, like),
                t,
            })
        };
        let first = Sub {
            e: piece(&z.e, z.cl.e.span, z.cl.e.t)?,
            g: piece(&z.g, z.cl.g.span, z.cl.g.t)?,
        };
        let mut subs = vec![first];
        let mut gap = gap_of(&subs, like)?;
        let mut size = f64::INFINITY;
        let mut stalled = 0;
        for _ in 0..MAX_ROUNDS {
            let off = if gap[0] > 0.0 {
                gap[0]
            } else if gap[1] < 0.0 {
                -gap[1]
            } else {
                0.0
            };
            // The sign certain with a margin, or the pieces no smaller at
            // this level's precision.
            if off > gap[1] - gap[0] || stalled >= STALL {
                break;
            }
            let mut next = Vec::new();
            let mut most = 0.0f64;
            for s in &subs {
                // The larger piece halves.
                let (se, sg) = (s.e.size()?, s.g.size()?);
                let halves: Vec<Sub<S>> = if se >= sg {
                    s.e.halves()
                        .into_iter()
                        .map(|e| Sub { e, g: s.g.clone() })
                        .collect()
                } else {
                    s.g.halves()
                        .into_iter()
                        .map(|g| Sub { e: s.e.clone(), g })
                        .collect()
                };
                for sub in halves {
                    work.boxes = work.boxes.saturating_add(1);
                    if !sub.apart(like)? {
                        let n = sub.e.size()? + sub.g.size()?;
                        if n > most {
                            most = n;
                        }
                        next.push(sub);
                    }
                }
            }
            if next.len() > PRUNE_FROM {
                // Many kept: drop those whose Krawczyk image is certified
                // apart from them (no crossing there; [`Self::newton`]).
                if let Some(k) = Kraw::of(z, like) {
                    let n = u32::try_from(next.len()).unwrap_or(u32::MAX);
                    work.boxes = work.boxes.saturating_add(n);
                    next.retain(|sub| {
                        let x = sub.params(like);
                        k.step(&x, like).is_none_or(|n| !apart_params(&n, &x))
                    });
                }
            }
            if next.is_empty() {
                return None;
            }
            if next.len() > MAX_SUBS {
                // Near the level's rounding the hulls stop parting the
                // halves: the sub-boxes kept so far hold the crossing.
                break;
            }
            subs = next;
            gap = gap_of(&subs, like)?;
            stalled = if most >= size { stalled + 1 } else { 0 };
            if most < size {
                size = most;
            }
        }
        Some((subs, gap))
    }

    /// The gap `UP·(C_g − C_e)` at the crossing by Krawczyk steps at
    /// `like`'s level from each kept sub-box (module docs): every root of
    /// the shadow difference in a box `X` lies in its Krawczyk image
    /// `K(X)` (with any centre `m ∈ X` and any matrix `Y`), so a sub-box
    /// whose image is certified apart from it holds no crossing and is
    /// dropped, and the crossing stays in each image, which shrinks
    /// quadratically about it where the shadows are transversal; then the
    /// gap over what is kept, enclosed. `None` where nothing is narrower
    /// than the sub-boxes.
    fn newton<S: Curved>(
        &self,
        z: &ZAt<'_>,
        subs: &[Sub<S>],
        like: &S,
        work: &mut LevelWork,
    ) -> Option<S> {
        let k = Kraw::of(z, like)?;
        let mut kept: Vec<[S; 2]> = Vec::new();
        for sub in subs {
            let mut x = sub.params(like);
            let mut root = true;
            for _ in 0..NEWTON_STEPS {
                work.boxes = work.boxes.saturating_add(1);
                let Some(next) = k.step(&x, like) else {
                    break;
                };
                if apart_params(&next, &x) {
                    root = false;
                    break;
                }
                if !(0..2).all(|i| width(&next[i]) < width(&x[i])) {
                    break;
                }
                x = next;
            }
            if root {
                kept.push(x);
            }
        }
        match kept.as_slice() {
            [] => None,
            [x] => Some(k.gap(x)),
            many => {
                let [lo, hi] = many
                    .iter()
                    .map(|x| k.gap(x).enclose())
                    .fold([f64::INFINITY, f64::NEG_INFINITY], hull);
                Some(between(lo, hi, like))
            }
        }
    }

    /// The edge–edge orders of a curved cluster at `like`'s level, over
    /// its kept sub-boxes (module docs).
    fn curved_orders<S: Curved>(
        &self,
        z: &ZAt<'_>,
        like: &S,
        work: &mut LevelWork,
    ) -> Option<[S; ORDERS]> {
        let (subs, _) = self.crossing(z, like, work)?;
        let mut out = [[f64::INFINITY, f64::NEG_INFINITY]; ORDERS];
        for s in &subs {
            let te = between(s.e.t[0], s.e.t[1], like);
            let ts = between(s.g.t[0], s.g.t[1], like);
            let re = self.model.edge_rates(Operand::A, z.x.e, z.cl.e.span, &te);
            let rg = self.model.edge_rates(Operand::B, z.x.g, z.cl.g.span, &ts);
            let deltas: [[S; 3]; ORDERS] =
                std::array::from_fn(|k| std::array::from_fn(|i| rg[k][i].sub(&re[k][i])));
            let ord = z_order_cone(&diffs(&s.e.c)?, &diffs(&s.g.c)?, &deltas)?;
            for (o, n) in out.iter_mut().zip(ord) {
                *o = hull(*o, n);
            }
        }
        Some(out.map(|[lo, hi]| between(lo, hi, like)))
    }
}

/// Kept sub-boxes past which each round drops those that Krawczyk's
/// image shows hold no crossing.
const PRUNE_FROM: usize = 4;

/// A cluster's two spans at a level, for Krawczyk steps on the shadow
/// difference `D(t, s) = shadow(C_e(t) − C_g(s))`.
struct Kraw<S> {
    ce: HomCurve<S, 3>,
    cg: HomCurve<S, 3>,
    de: HomCurve<S, 3>,
    dg: HomCurve<S, 3>,
    axes: [[S; 3]; 3],
}

impl<S: Curved> Kraw<S> {
    fn of(z: &ZAt<'_>, like: &S) -> Option<Kraw<S>> {
        let ce = z.e.span(z.cl.e.span, like)?;
        let cg = z.g.span(z.cl.g.span, like)?;
        let v = |d: DVec3| [d.x, d.y, d.z].map(|c| S::from_f64(c, like));
        Some(Kraw {
            de: ce.derivative(),
            dg: cg.derivative(),
            ce,
            cg,
            axes: [v(R_HAT), v(NU), v(UP)],
        })
    }

    fn shadow(&self, p: &[S; 3]) -> [S; 2] {
        [dot(&self.axes[0], p), dot(&self.axes[1], p)]
    }

    /// `K(x) = m − Y·D(m) + (I − Y·J(x))·(x − m)`, `m` the centre of `x`
    /// and `Y` the inverse of `J` at it; `None` where that `J` isn't
    /// certified invertible.
    fn step(&self, x: &[S; 2], like: &S) -> Option<[S; 2]> {
        let m = [x[0].center(), x[1].center()];
        let d_m = self.shadow(&sub(&self.ce.eval(&m[0]), &self.cg.eval(&m[1])));
        let (je, jg) = (
            self.shadow(&derivative(&self.ce, &self.de, &x[0])),
            self.shadow(&derivative(&self.cg, &self.dg, &x[1])),
        );
        let j = [[je[0].clone(), jg[0].neg()], [je[1].clone(), jg[1].neg()]];
        let c = j.each_ref().map(|row| row.each_ref().map(Curved::center));
        let det = c[0][0].mul(&c[1][1]).sub(&c[0][1].mul(&c[1][0]));
        if !matches!(det.sign(), Some(Ordering::Greater | Ordering::Less)) {
            return None;
        }
        let y = [
            [c[1][1].div(&det), c[0][1].neg().div(&det)],
            [c[1][0].neg().div(&det), c[0][0].div(&det)],
        ]
        .map(|row| row.map(|e| e.center()));
        let off = [x[0].sub(&m[0]), x[1].sub(&m[1])];
        Some(std::array::from_fn(|i| {
            let yd = y[i][0].mul(&d_m[0]).add(&y[i][1].mul(&d_m[1]));
            let mut out = m[i].sub(&yd);
            for (jx, o) in off.iter().enumerate() {
                let yj = y[i][0].mul(&j[0][jx]).add(&y[i][1].mul(&j[1][jx]));
                let id = S::int(u32::from(i == jx), like);
                out = out.add(&id.sub(&yj).mul(o));
            }
            out
        }))
    }

    /// The gap `UP·(C_g(s) − C_e(t))` over `x`.
    fn gap(&self, x: &[S; 2]) -> S {
        dot(
            &self.axes[2],
            &sub(&self.cg.eval(&x[1]), &self.ce.eval(&x[0])),
        )
    }
}

/// Whether two parameter boxes are certified apart in either parameter.
fn apart_params<S: Scalar>(a: &[S; 2], b: &[S; 2]) -> bool {
    (0..2).any(|i| {
        let (x, y) = (a[i].enclose(), b[i].enclose());
        x[1] < y[0] || y[1] < x[0]
    })
}

/// Krawczyk steps after the halving: each about doubles the digits.
const NEWTON_STEPS: usize = 6;

/// The width of a value's enclosure (`+∞` uncertified).
fn width<S: Scalar>(x: &S) -> f64 {
    let [lo, hi] = x.enclose();
    hi - lo
}

/// The derivative of `c`'s point at `t`, `(W·P′ − W′·P)/W²`, `dc` its
/// derivative curve.
fn derivative<S: Scalar>(c: &HomCurve<S, 3>, dc: &HomCurve<S, 3>, t: &S) -> [S; 3] {
    let (p, w) = c.eval_hom(t);
    let (dp, dw) = dc.eval_hom(t);
    let w2 = w.mul(&w);
    std::array::from_fn(|k| w.mul(&dp[k]).sub(&dw.mul(&p[k])).div(&w2))
}

fn dot<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> S {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

fn sub<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> [S; 3] {
    std::array::from_fn(|k| a[k].sub(&b[k]))
}

/// The most rounds of halving a curved cluster's crossing is looked for
/// in: past `f64`'s parameters at any level.
const MAX_ROUNDS: usize = 160;

/// Rounds of halving without the largest kept sub-box shrinking at all
/// after which the level's precision is taken as reached.
const STALL: usize = 8;

/// The most sub-boxes kept at once: a transversal crossing keeps a few,
/// until the level's rounding stops the hulls parting them.
const MAX_SUBS: usize = 256;

/// A piece of an edge's span and its parameters there.
#[derive(Clone, Debug)]
struct Piece<S> {
    c: HomCurve<S, 3>,
    t: [f64; 2],
}

impl<S: Scalar> Piece<S> {
    /// The two halves, the lower first.
    fn halves(&self) -> [Piece<S>; 2] {
        let [l, r] = self.c.split_half();
        let m = 0.5 * self.t[0] + 0.5 * self.t[1];
        [
            Piece {
                c: l,
                t: [self.t[0], m],
            },
            Piece {
                c: r,
                t: [m, self.t[1]],
            },
        ]
    }

    /// The summed widths of its certified box in 3D.
    fn size(&self) -> Option<f64> {
        let b = self.c.control_box()?;
        Some(b.iter().fold(0.0, |s, [lo, hi]| s + (hi - lo)))
    }

    /// The certified hull in the frame `(r̂·x, ν·x, UP·x)`.
    fn frame(&self, like: &S) -> Option<[[f64; 2]; 3]> {
        let v = |d: DVec3| [d.x, d.y, d.z].map(|c| S::from_f64(c, like));
        self.c.map_linear(&[v(R_HAT), v(NU), v(UP)]).control_box()
    }
}

/// A sub-box of a cluster: a piece of each edge.
#[derive(Clone, Debug)]
struct Sub<S> {
    e: Piece<S>,
    g: Piece<S>,
}

impl<S: Curved> Sub<S> {
    /// Its parameters `[t, s]` as enclosures at the level.
    fn params(&self, like: &S) -> [S; 2] {
        [
            between(self.e.t[0], self.e.t[1], like),
            between(self.g.t[0], self.g.t[1], like),
        ]
    }
}

impl<S: Scalar> Sub<S> {
    /// Whether the two pieces' shadows' hulls are certified apart.
    fn apart(&self, like: &S) -> Option<bool> {
        let (e, g) = (self.e.frame(like)?, self.g.frame(like)?);
        Some((0..2).any(|k| e[k][1] < g[k][0] || g[k][1] < e[k][0]))
    }
}

/// The gap `UP·(C_g − C_e)` over every sub-box: `[lo, hi]`, rounded out.
fn gap_of<S: Scalar>(subs: &[Sub<S>], like: &S) -> Option<[f64; 2]> {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for s in subs {
        let (e, g) = (s.e.frame(like)?[2], s.g.frame(like)?[2]);
        // Rounded out: `−up(−x)` is a lower bound of a rounded `x`.
        [lo, hi] = hull([lo, hi], [-up(e[1] - g[0]), up(g[1] - e[0])]);
    }
    (lo.is_finite() && hi.is_finite()).then_some([lo, hi])
}

/// The affine control point differences of a piece, every pair: their
/// positive combinations hold its tangents (positive weights).
fn diffs<S: Scalar>(c: &HomCurve<S, 3>) -> Option<Vec<[S; 3]>> {
    let w = c.weights().coefs();
    if w.iter().any(|x| x.sign() != Some(Ordering::Greater)) {
        return None;
    }
    let pts: Vec<[S; 3]> = (0..w.len())
        .map(|i| [0, 1, 2].map(|k| c.coords()[k].coefs()[i].div(&w[i])))
        .collect();
    let mut out = Vec::new();
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            out.push([0, 1, 2].map(|k| pts[j][k].sub(&pts[i][k])));
        }
    }
    Some(out)
}

/// Whether `p` lies on the segment from `a` to `b` at a parameter in
/// `[lo, hi]`, exactly.
fn on_segment(a: [f64; 3], b: [f64; 3], p: [f64; 3], [lo, hi]: [f64; 2]) -> bool {
    let (a, b, p) = (a.map(rat), b.map(rat), p.map(rat));
    let t: [Rational; 3] = std::array::from_fn(|k| &b[k] - &a[k]);
    let w: [Rational; 3] = std::array::from_fn(|k| &p[k] - &a[k]);
    let zero = Rational::from(0u32);
    if qcross(&t, &w).iter().any(|x| *x != zero) {
        return false;
    }
    let (tw, tt) = (qdot(&t, &w), qdot(&t, &t));
    if tt == zero {
        return w.iter().all(|x| *x == zero);
    }
    let s = tw / tt;
    rat(lo) <= s && s <= rat(hi)
}

/// The least interval holding `a` and `b` (compared and selected: no
/// `f64::min`).
fn hull(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [
        if b[0] < a[0] { b[0] } else { a[0] },
        if b[1] > a[1] { b[1] } else { a[1] },
    ]
}

fn qdot(a: &[Rational; 3], b: &[Rational; 3]) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn qcross(a: &[Rational; 3], b: &[Rational; 3]) -> [Rational; 3] {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

impl<M: HeightModel + XGeometry> HeightGeometry for XHeights<'_, M> {
    type Flat = FlatHeight;

    fn value(&self, h: HKey) -> KeyValue<FlatHeight> {
        let Some(z) = self.cluster(h) else {
            return self.base().value(h);
        };
        match z.cl.row {
            // Both edges on the curve at the root: exactly 0.
            ZRow::Root { .. } => KeyValue::Exact(Ok(Ordering::Equal)),
            ZRow::EdgeEdge if Self::flat(&z) => self.base().value(h),
            // A vertex on a vertex of two curves: not decided here
            // (module docs, "Shared corners").
            ZRow::EdgeEdge if Self::curved_corner(&z) => KeyValue::Curved,
            // An end on the other edge: exactly 0.
            ZRow::EdgeEdge if Self::incident(&z) => KeyValue::Exact(Ok(Ordering::Equal)),
            ZRow::EdgeEdge => KeyValue::Curved,
        }
    }

    fn curved_value<S: Curved>(&self, h: HKey, like: &S, work: &mut LevelWork) -> Option<S> {
        let Some(z) = self.cluster(h) else {
            return self.base().curved_value(h, like, work);
        };
        if z.cl.row != ZRow::EdgeEdge || Self::flat(&z) || Self::curved_corner(&z) {
            return None;
        }
        let (subs, [lo, hi]) = self.crossing(&z, like, work)?;
        let len = S::from_f64(UP.dot(UP), like).sqrt();
        let boxed = between(lo, hi, like);
        // Where the sub-boxes leave the sign open, Newton's steps at the
        // level may close it: the narrower enclosure.
        let nv = self.newton(&z, &subs, like, work);
        let v = match nv {
            Some(v) if width(&v) < width(&boxed) => v,
            _ => boxed,
        };
        // Widened by the target: decided only beyond it (a geometric
        // value, never a tie; module docs, "Near ties").
        Some(v.div(&len).widened(self.near))
    }

    fn row(&self, k: HeightKey) -> AnchorRow<FlatHeight> {
        let z = match (k.anchor, self.cluster(k.h)) {
            (Anchor::Edge(..), Some(z)) => z,
            _ => return self.base().row(k),
        };
        match z.cl.row {
            ZRow::EdgeEdge if Self::flat(&z) => self.base().row(k),
            ZRow::EdgeEdge | ZRow::Root { .. } => AnchorRow::Curved,
        }
    }

    fn curved_row<S: Curved>(
        &self,
        k: HeightKey,
        like: &S,
        work: &mut LevelWork,
    ) -> Option<[S; ORDERS]> {
        let z = match (k.anchor, self.cluster(k.h)) {
            (Anchor::Edge(..), Some(z)) => z,
            _ => return self.base().curved_row(k, like, work),
        };
        match z.cl.row {
            ZRow::Root { .. } => self.root_orders(&z, like),
            ZRow::EdgeEdge if Self::flat(&z) => None,
            ZRow::EdgeEdge => self.curved_orders(&z, like, work),
        }
    }

    fn facing_only<S: Curved>(&self, h: HKey, like: &S) -> bool {
        self.base().facing_only(h, like)
    }

    fn curved_fold<S: Curved>(&self, k: HeightKey, like: &S) -> Option<S> {
        self.base().curved_fold(k, like)
    }

    fn geometric(&self, k: HeightKey) -> bool {
        // A curved edge–edge value away from exact incidence is decided by
        // its sign, never a tie: the column hits it agrees with are exact.
        match self.cluster(k.h) {
            Some(z) if z.cl.row == ZRow::EdgeEdge && !Self::flat(&z) => {
                Self::curved_corner(&z) || !Self::incident(&z)
            }
            // An exact 0 at the root, the orders deciding: never a truth.
            Some(z) if matches!(z.cl.row, ZRow::Root { .. }) => false,
            _ => self.base().geometric(k),
        }
    }
}
