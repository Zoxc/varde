//! The runs: placements of two solids, each counted ([`World::count`])
//! and checked against the ray oracle, on several threads; the tallies
//! of agreements, refusals by cause, the oracle's ties, and the
//! counting's precision levels.

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use glam::DVec3;
use varde_testkit::ray_oracle::{Chain, Coedge, ExactFace, Face, Shift, UvChain};
use varde_testkit::rng::Rng;

use super::solids::{Make, Shape, Solid};
use super::turned::{TurnedEdges, fold, group_of};
use super::world::{Answers, Cause, Config, Refusal, Stats, World};
use crate::topo::brep_min::{EdgeId, FaceId};

/// A placement: the two shapes, how each is built, and `A`'s direction.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Placement {
    pub a: Shape,
    pub b: Shape,
    pub make_a: Make,
    pub make_b: Make,
    pub d: DVec3,
}

impl Placement {
    /// The random rigid placement of seed `seed`: each shape of a random
    /// kind and size about 2, `B` near the origin, `A` overlapping it,
    /// every p-curve perturbed by up to `10⁻⁶` mm, vertex ids shuffled.
    pub(super) fn random(seed: u64) -> Placement {
        let mut rng = Rng::new(seed);
        let kb = (rng.next_u64() % 3) as u32;
        let ka = (rng.next_u64() % 3) as u32;
        let at = rng_point(&mut rng, 0.5);
        let b = Shape::random(&mut rng, kb, at, 2.0);
        let at = rng_point(&mut rng, 1.5);
        let a = Shape::random(&mut rng, ka, at, 2.0);
        let make = |rng: &mut Rng| Make {
            pert: rng.range(5e-7, 1e-6),
            pert_seed: rng.next_u64(),
            ids: Some(rng.next_u64()),
            ..Make::PLAIN
        };
        let (make_a, make_b) = (make(&mut rng), make(&mut rng));
        Placement {
            a,
            b,
            make_a,
            make_b,
            d: rng.direction(),
        }
    }

    /// Whether both shapes' p-curves are unperturbed (the family of ties
    /// exact in the counting's records): only then are the faces on
    /// coordinate planes exact, tile, p-curves and edges agreeing bit for
    /// bit ([`TurnedEdges`]).
    pub(super) fn unperturbed(&self) -> bool {
        self.make_a.pert == 0.0 && self.make_b.pert == 0.0
    }
}

fn rng_point(rng: &mut Rng, extent: f64) -> DVec3 {
    DVec3::new(
        rng.range(-extent, extent),
        rng.range(-extent, extent),
        rng.range(-extent, extent),
    )
}

/// A solid's faces as the oracle reads them.
pub(super) fn oracle_faces(s: &Solid) -> Vec<ExactFace> {
    s.faces()
        .into_iter()
        .map(|f| ExactFace::new(&oracle_face(s, f)))
        .collect()
}

pub(super) fn oracle_face(s: &Solid, f: FaceId) -> Face {
    let b = &s.brep;
    let surf = b.surface(f);
    let r = surf.rect();
    let mut tiles = Vec::new();
    for i in r.u.clone() {
        for j in r.v.clone() {
            tiles.push(([i, j], *surf.tile(i, j).expect("a tile").net()));
        }
    }
    let loops = b
        .loops(f)
        .map(|l| {
            b.loop_coedges(l)
                .map(|c| {
                    let e = b.edge(c);
                    let p = b.pcurve(c);
                    Coedge {
                        pcurve: UvChain {
                            spans: p.spans().to_vec(),
                            knots: p.knots().to_vec(),
                        },
                        curve: chain(s, e),
                        t_range: b.t_range(e),
                        reversed: b.reversed(c),
                    }
                })
                .collect()
        })
        .collect();
    Face { tiles, loops }
}

fn chain(s: &Solid, e: EdgeId) -> Chain {
    let c = s.brep.curve(e);
    Chain {
        spans: c.spans().to_vec(),
        knots: c.knots().to_vec(),
    }
}

/// The oracle's answers for a placement, in the counting's layout, and
/// whether any query was a tie (counted shifted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Truth {
    pub winding: [Vec<i64>; 2],
    pub x12: Vec<Vec<i64>>,
    pub x21: Vec<Vec<i64>>,
    pub ties: u64,
}

const UP: [f64; 3] = [2.0, 3.0, 32.0];

/// The oracle's casts: windings by upward rays from `A`'s vertices
/// through `B`'s faces and downward ones from `B`'s through `A`'s, the
/// edges' crossings with the faces, `A` moved by the tie shift.
pub(super) fn truth(
    a: &Solid,
    b: &Solid,
    d: DVec3,
    fa: &[ExactFace],
    fb: &[ExactFace],
) -> Result<Truth, String> {
    truth_at(a, b, &Shift::tie(d.to_array(), 1), fa, fb)
}

/// [`truth`] with `A` moved by `tie`.
pub(super) fn truth_at(
    a: &Solid,
    b: &Solid,
    tie: &Shift,
    fa: &[ExactFace],
    fb: &[ExactFace],
) -> Result<Truth, String> {
    let tie = tie.clone();
    let none = Shift::NONE;
    let down = UP.map(|x| -x);
    let mut ties = 0;
    let mut tally = |r: Result<varde_testkit::ray_oracle::Crossings, _>| -> Result<i64, String> {
        let c = r.map_err(|e| format!("{e:?}"))?;
        ties += u64::from(c.tie);
        Ok(c.count)
    };
    let mut wa = Vec::new();
    for v in a.verts() {
        let q = a.brep.pos(v);
        let mut w = 0;
        for f in fb {
            w += tally(f.ray(&none, q, &tie, UP))?;
        }
        wa.push(w);
    }
    let mut wb = Vec::new();
    for v in b.verts() {
        let q = b.brep.pos(v);
        let mut w = 0;
        for f in fa {
            w += tally(f.ray(&tie, q, &none, down))?;
        }
        wb.push(w);
    }
    let mut x12 = Vec::new();
    for e in a.edges() {
        let c = chain(a, e);
        let mut row = Vec::new();
        for f in fb {
            row.push(tally(f.curve(&none, &c, &tie))?);
        }
        x12.push(row);
    }
    let mut x21 = Vec::new();
    for g in b.edges() {
        let c = chain(b, g);
        let mut row = Vec::new();
        for f in fa {
            row.push(tally(f.curve(&tie, &c, &none))?);
        }
        x21.push(row);
    }
    Ok(Truth {
        winding: [wa, wb],
        x12,
        x21,
        ties,
    })
}

/// The oracle's answers with `A` moved by `2^-28·d` and by `−2^-28·d`
/// ([`near_shift`], about `4·10⁻⁹` mm at unit size): past the faces'
/// rounding misfit (a plane tile's patch passes within ~`10⁻¹⁰` mm of
/// its key's plane and its vertices at unit size, not through them) and
/// the records' error target `2^-30`, within every band. Where the
/// oracle's answers differ between the two, the placement is within that
/// distance of a tie.
pub(super) fn truths_near(
    a: &Solid,
    b: &Solid,
    d: DVec3,
    fa: &[ExactFace],
    fb: &[ExactFace],
) -> Result<[Truth; 2], String> {
    let at = |sign: f64| truth_at(a, b, &near_shift(a, b, d * sign), fa, fb);
    Ok([at(1.0)?, at(-1.0)?])
}

/// `2^-28·d` relative to the data: `d` scaled to max-norm 1, times
/// `2^-28` of the binade of the solids' largest coordinate (the misfit
/// and the records' error target scale with it), with
/// `Shift::always` (every query counted shifted).
pub(super) fn near_shift(a: &Solid, b: &Solid, d: DVec3) -> Shift {
    let (ma, mb) = (a.brep.max_coord(), b.brep.max_coord());
    let m = if ma > mb { ma } else { mb };
    // The binade of `m`: its exponent bits (subnormals and zero as the
    // smallest normal binade).
    let e = ((m.to_bits() >> 52) & 0x7ff).max(1) as i64 - 1023;
    let n = d.to_array().map(f64::abs);
    let n = n.into_iter().fold(0.0, |m, x| if x > m { x } else { m });
    Shift::always((d / n).to_array(), e - 28)
}

/// Whether two of the oracle's answers are the same (their ties aside),
/// the crossing counts at crossed turned edges by their sums.
fn same(x: &Truth, y: &Truth, turned: &TurnedEdges) -> bool {
    x.winding == y.winding
        && fold(&turned.x12, &x.x12) == fold(&turned.x12, &y.x12)
        && fold(&turned.x21, &x.x21) == fold(&turned.x21, &y.x21)
}

/// Whether two of the counting's answers are the same, the crossing
/// counts at crossed turned edges by their sums.
fn same_answers(x: &Answers, y: &Answers, turned: &TurnedEdges) -> bool {
    x.winding == y.winding
        && x.column == y.column
        && fold(&turned.x12, &x.x12) == fold(&turned.x12, &y.x12)
        && fold(&turned.x21, &x.x21) == fold(&turned.x21, &y.x21)
}

/// How a placement went.
#[derive(Clone, Debug)]
pub(super) enum Outcome {
    /// The counting answered and the oracle agrees.
    Agreed { ties: u64 },
    /// The counting answered and the oracle disagrees.
    Mismatch(String),
    /// The counting refused.
    Refused(Cause, String),
    /// The oracle couldn't settle a query.
    Unresolved(String),
    /// The counting's answer differs from the oracle's, but equals the
    /// oracle's with `A` moved by `2^-28·d` or by `−2^-28·d`: the
    /// oracle's answer changes within that distance (between its tie
    /// shift and one side, or between the sides), inside the misfit
    /// between the counting's flat records (vertices, key planes) and
    /// the tiles the oracle casts against.
    TieScale(String),
    /// The oracle's winding of a vertex lies outside `{0, 1}`: the
    /// operand's own ribbon-closed faces aren't embedded at it (a lobe,
    /// where two faces at an edge overshoot each other by a seam), so no
    /// exact answer is a solid's. The counting took the lobe fallback
    /// (the edge rule for the vertex's hits near the edge) and answered.
    /// Counted apart from mismatches.
    OperandLobe(String),
    /// The counting's answers equal the oracle's but for per-face
    /// crossing counts at an edge between rounded faces that an edge of
    /// the other operand crosses exactly ([`TurnedEdges`]): those follow
    /// the faces' misfit-width ribbons, not `d`, and are tie-scale; their
    /// sums over the edge's faces agree. Counted apart from mismatches
    /// and from the tie-scale results the family of exact ties forbids.
    TurnedEdgeFaces(String),
    /// The counting's answers with `d` and with `−d` both equal the
    /// oracle's at its tie shift, which doesn't flip with `d`, while the
    /// oracle's answers at `±2^-28·d` differ: the placement's exact gap
    /// is non-zero and below that probe (a vertex on a silhouette, a hair
    /// off it by rounding), so only the probe flips (found by the `±d`
    /// check). Counted apart from agreements and disagreements.
    OracleOnlyFlip,
}

/// Where the counting's answers differ from the oracle's, if anywhere:
/// the crossing counts at crossed turned edges ([`TurnedEdges`]) by their
/// sum over the edge's faces.
pub(super) fn compare(c: &Answers, t: &Truth, turned: &TurnedEdges) -> Option<String> {
    let mut out = Vec::new();
    for (k, op) in ["A", "B"].into_iter().enumerate() {
        for (v, (w, o)) in c.winding[k].iter().zip(&t.winding[k]).enumerate() {
            if w != o {
                out.push(format!("winding {op} v{v}: counted {w}, oracle {o}"));
            }
        }
        for (v, (w, o)) in c.column[k].iter().zip(&t.winding[k]).enumerate() {
            if w != o {
                out.push(format!("column {op} v{v}: counted {w}, oracle {o}"));
            }
        }
    }
    for (name, cx, tx, groups) in [
        ("x12", &c.x12, &t.x12, &turned.x12),
        ("x21", &c.x21, &t.x21, &turned.x21),
    ] {
        let (cx, tx) = (fold(groups, cx), fold(groups, tx));
        for (e, (cr, tr)) in cx.iter().zip(&tx).enumerate() {
            for (f, (x, y)) in cr.iter().zip(tr).enumerate() {
                if x != y {
                    let faces = group_of(groups, e, f)
                        .map_or_else(|| format!("f{f}"), |g| format!("faces {g:?} summed"));
                    out.push(format!("{name} e{e} {faces}: counted {x}, oracle {y}"));
                }
            }
        }
    }
    (!out.is_empty()).then(|| out.join("; "))
}

/// Where the counting's per-face crossing counts at crossed turned edges
/// differ from the oracle's, if anywhere (tie-scale: the sums are checked
/// by [`compare`]).
pub(super) fn per_face(c: &Answers, t: &Truth, turned: &TurnedEdges) -> Option<String> {
    let mut out = Vec::new();
    for (name, cx, tx, groups) in [
        ("x12", &c.x12, &t.x12, &turned.x12),
        ("x21", &c.x21, &t.x21, &turned.x21),
    ] {
        for (e, gs) in groups.iter().enumerate() {
            for &f in gs.iter().flatten() {
                let (x, y) = (cx[e][f], tx[e][f]);
                if x != y {
                    out.push(format!("{name} e{e} f{f}: counted {x}, oracle {y}"));
                }
            }
        }
    }
    (!out.is_empty()).then(|| out.join("; "))
}

/// Whether some winding of the oracle's lies outside `{0, 1}`: an
/// operand lobe ([`Outcome::OperandLobe`]).
pub(super) fn lobed(t: &Truth) -> bool {
    t.winding.iter().flatten().any(|w| !(0..=1).contains(w))
}

/// The counting run again with `A` moving along `−d`, checked against
/// the run with `d` and the oracle's answers with `A` moved by
/// `±2^-28·d`: where a placement is tied in the counting's own records
/// the two counts must differ exactly where those two answers do.
#[derive(Clone, Debug)]
pub(super) enum Flip {
    /// The counting refused with `−d`.
    Refused(Cause),
    /// The oracle couldn't settle a query at `±2^-28·d`.
    Unresolved(String),
    Checked {
        /// The counting's answers with `d` and with `−d` differ.
        counted: bool,
        /// The oracle's answers at `2^-28·d` and `−2^-28·d` differ.
        truth: bool,
        /// The counting's answers with `d` and with `−d` each equal the
        /// oracle's on their side.
        sides: [bool; 2],
    },
    /// Flip only in the oracle, gap below the probe: the oracle's answers
    /// at `±2^-28·d` differ, its answers at its tie shift along `d` and
    /// along `−d` are the same (the exact gap isn't zero), and the
    /// counting's answers with `d` and with `−d` both equal them.
    OracleOnly,
}

/// A placement's check: its outcome, the counting's answers where it
/// gave them, the `±d` check where asked for and answered, and how many
/// groups of faces at crossed turned edges ([`TurnedEdges`]) it checked
/// by their sums.
#[derive(Clone, Debug)]
pub(super) struct Checked {
    pub outcome: Outcome,
    pub answers: Option<Answers>,
    pub flip: Option<Flip>,
    pub turned: usize,
}

/// A placement's solids, its counting and its check, with the
/// counting's answers where it gave them.
pub(super) fn check_answers(
    p: &Placement,
    cfg: &Config,
    stats: &mut Stats,
) -> (Outcome, Option<Answers>) {
    let c = check_with(p, cfg, stats, false);
    (c.outcome, c.answers)
}

/// How a placement is counted: `A` against `B`, `A` moving along `d`.
/// [`counting`] is the counting under test; the tests of the checks
/// swap in deliberately broken ones.
pub(super) type Counting =
    fn(&Solid, &Solid, DVec3, &Config, &mut Stats) -> Result<Answers, Refusal>;

/// The counting from the records ([`World::count`]).
pub(super) fn counting(
    a: &Solid,
    b: &Solid,
    d: DVec3,
    cfg: &Config,
    stats: &mut Stats,
) -> Result<Answers, Refusal> {
    World::new(a, b, d).count(cfg, stats)
}

/// [`check_answers`], and with `flip` the `±d` check ([`Flip`]) where
/// the counting answered.
pub(super) fn check_with(p: &Placement, cfg: &Config, stats: &mut Stats, flip: bool) -> Checked {
    check_counted(p, cfg, stats, flip, counting)
}

/// [`check_with`] with the placement counted by `count`.
pub(super) fn check_counted(
    p: &Placement,
    cfg: &Config,
    stats: &mut Stats,
    flip: bool,
    count: Counting,
) -> Checked {
    let (a, b) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
    let turned = TurnedEdges::of(&a, &b, p.unperturbed());
    let answers = match count(&a, &b, p.d, cfg, stats) {
        Ok(x) => x,
        Err(r) => {
            return Checked {
                outcome: Outcome::Refused(r.cause, r.at),
                answers: None,
                flip: None,
                turned: turned.count(),
            };
        }
    };
    let (fa, fb) = (oracle_faces(&a), oracle_faces(&b));
    let near = OnceCell::new();
    let near = || near.get_or_init(|| truths_near(&a, &b, p.d, &fa, &fb));
    let (mut outcome, t) = judge(p, &a, &b, &answers, &turned, &fa, &fb, &near);
    let flip = (flip && !matches!(outcome, Outcome::Unresolved(_))).then(|| {
        // Its own tallies: the run's are the base counts'.
        let minus = count(&a, &b, -p.d, cfg, &mut Stats::default());
        match (minus, near()) {
            (Err(r), _) => Flip::Refused(r.cause),
            (_, Err(e)) => Flip::Unresolved(e.clone()),
            (Ok(m), Ok([up, down])) => {
                let counted = !same_answers(&answers, &m, &turned);
                let truth = !same(up, down, &turned);
                // Only the probe flips: the counting doesn't, both its
                // answers are the oracle's at its tie shift, and that
                // shift's answers don't flip with `d` (computed last: an
                // oracle run more, needed only here).
                let below = truth
                    && !counted
                    && t.as_ref().is_some_and(|t| {
                        compare(&answers, t, &turned).is_none()
                            && compare(&m, t, &turned).is_none()
                            && truth_at(&a, &b, &Shift::tie((-p.d).to_array(), 1), &fa, &fb)
                                .is_ok_and(|tm| same(t, &tm, &turned))
                    });
                if below {
                    Flip::OracleOnly
                } else {
                    Flip::Checked {
                        counted,
                        truth,
                        sides: [
                            compare(&answers, up, &turned).is_none(),
                            compare(&m, down, &turned).is_none(),
                        ],
                    }
                }
            }
        }
    });
    if matches!(flip, Some(Flip::OracleOnly)) && matches!(outcome, Outcome::Agreed { .. }) {
        outcome = Outcome::OracleOnlyFlip;
    }
    Checked {
        outcome,
        answers: Some(answers),
        flip,
        turned: turned.count(),
    }
}

/// The oracle's verdict on the counting's answers, and the oracle's
/// answers (with its tie shift) where it settled them; `near` gives the
/// oracle's answers at `±2^-28·d` ([`truths_near`]).
#[allow(
    clippy::too_many_arguments,
    reason = "the placement, its solids and their oracle faces, the answers, and the lazy probe"
)]
fn judge<'n>(
    p: &Placement,
    a: &Solid,
    b: &Solid,
    answers: &Answers,
    turned: &TurnedEdges,
    fa: &[ExactFace],
    fb: &[ExactFace],
    near: &dyn Fn() -> &'n Result<[Truth; 2], String>,
) -> (Outcome, Option<Truth>) {
    let t = match truth(a, b, p.d, fa, fb) {
        // Unsettled at the tie shift (coincident curves the shift leaves
        // degenerate) but settled past the misfit along `d`: the
        // counting's answers there are checked, tie-scale.
        Err(e) => {
            let along = match near() {
                Ok([up, _]) => compare(answers, up, turned).is_none(),
                Err(_) => false,
            };
            return if along {
                (
                    Outcome::TieScale(format!(
                        "(oracle unresolved at its tie shift, the oracle's at 2^-28·d) {e}"
                    )),
                    None,
                )
            } else {
                (Outcome::Unresolved(e), None)
            };
        }
        Ok(t) => t,
    };
    let o = verdict(answers, &t, turned, near);
    (o, Some(t))
}

/// [`judge`] given the oracle's answers `t`.
fn verdict<'n>(
    answers: &Answers,
    t: &Truth,
    turned: &TurnedEdges,
    near: &dyn Fn() -> &'n Result<[Truth; 2], String>,
) -> Outcome {
    let differs = compare(answers, t, turned);
    // An operand lobe: no exact answer is a solid's; the counting must
    // have taken the lobe fallback there, or read the lobe's point as
    // the ideal faces do (a lobe within the rounding misfit at a vertex
    // the query point lies on exactly, which the counting reads by the
    // vertex's germ), its answers then the oracle's past the misfit, at
    // `2^-28·d` or `−2^-28·d`.
    if lobed(t) {
        return match (answers.lobes, differs) {
            (0, Some(m)) => {
                let past = match near() {
                    Ok([up, down]) => {
                        compare(answers, up, turned).is_none()
                            || compare(answers, down, turned).is_none()
                    }
                    Err(_) => false,
                };
                if past {
                    Outcome::OperandLobe(format!("0 fallbacks, the oracle's at ±2^-28·d; {m}"))
                } else {
                    Outcome::Mismatch(format!("operand lobe without a lobe fallback: {m}"))
                }
            }
            (n, m) => Outcome::OperandLobe(format!(
                "{n} fallbacks; {}",
                m.unwrap_or_else(|| "agrees".into())
            )),
        };
    }
    let Some(m) = differs else {
        return match per_face(answers, t, turned) {
            None => Outcome::Agreed { ties: t.ties },
            Some(x) => Outcome::TurnedEdgeFaces(x),
        };
    };
    // Within the faces' rounding misfit the counting surface the oracle
    // casts against and the records' flat geometry differ, and the
    // oracle's tie shift lands inside that misfit: where the counting
    // agrees with the oracle at `2^-28·d` or `−2^-28·d` but not at the
    // shift, the oracle's answer changes within that distance (between
    // the sides, or between its shift and both sides) and the placement
    // is within the misfit of a tie.
    let [up, down] = match near() {
        Err(e) => return Outcome::Unresolved(e.clone()),
        Ok(x) => x,
    };
    if compare(answers, up, turned).is_none() || compare(answers, down, turned).is_none() {
        let tie = if t.ties > 0 {
            "(oracle tie)"
        } else {
            "(no oracle tie)"
        };
        Outcome::TieScale(format!("{tie} {m}"))
    } else {
        let near = if same(up, down, turned) {
            ""
        } else {
            "(within 2^-28 of a tie) "
        };
        Outcome::Mismatch(format!("{near}{m}"))
    }
}

/// The tallies of the `±d` checks of a run.
#[derive(Debug, Default)]
pub(super) struct FlipTally {
    /// Placements counted both ways (and settled by the oracle).
    pub both: u64,
    /// Of those, where the counting's answers differ with `d` and `−d`.
    pub counted: u64,
    /// Where the oracle's answers at `±2^-28·d` differ.
    pub truth: u64,
    /// Where both counts equal the oracle's on their side.
    pub sides: u64,
    /// Where the counting flips and the oracle doesn't, or the other way
    /// round: the seed, whether the placement's outcome is
    /// [`Outcome::OperandLobe`] (known: no exact answer there), and which
    /// flipped.
    pub differ: Vec<(u64, bool, String)>,
    /// Flips only in the oracle, gap below the probe ([`Flip::OracleOnly`]),
    /// by seed: not in `both` nor in `differ`.
    pub oracle_only: Vec<u64>,
    /// The counting refused with `−d`, by cause.
    pub refused: BTreeMap<String, u64>,
    pub unresolved: Vec<(u64, String)>,
}

impl FlipTally {
    fn add(&mut self, seed: u64, f: Flip, known: bool) {
        match f {
            Flip::Refused(c) => *self.refused.entry(cause_name(&c)).or_default() += 1,
            Flip::Unresolved(u) => self.unresolved.push((seed, u)),
            Flip::OracleOnly => self.oracle_only.push(seed),
            Flip::Checked {
                counted,
                truth,
                sides,
            } => {
                self.both += 1;
                self.counted += u64::from(counted);
                self.truth += u64::from(truth);
                self.sides += u64::from(sides == [true, true]);
                if counted != truth {
                    let which = if counted {
                        "the counting flips, the oracle doesn't"
                    } else {
                        "the oracle flips, the counting doesn't"
                    };
                    self.differ.push((seed, known, which.into()));
                }
            }
        }
    }

    /// The disagreements not at operand lobes.
    pub(super) fn unknown(&self) -> impl Iterator<Item = &(u64, bool, String)> {
        self.differ.iter().filter(|x| !x.1)
    }

    fn text(&self) -> String {
        let known = self.differ.iter().filter(|x| x.1).count();
        let seeds: Vec<u64> = self.unknown().take(12).map(|x| x.0).collect();
        let only: Vec<u64> = self.oracle_only.iter().take(12).copied().collect();
        let mut s = format!(
            "±d check: counted both ways {}; answers flip with d: counted {}, oracle at ±2^-28·d {}; agree {}, differ {} (at operand lobes {known}, others at seeds {seeds:?}); both sides equal the oracle's {}; flip only in the oracle, gap below the probe {} (seeds {only:?}); refused with −d {:?}; oracle unresolved {}\n",
            self.both,
            self.counted,
            self.truth,
            self.both - self.differ.len() as u64,
            self.differ.len(),
            self.sides,
            self.oracle_only.len(),
            self.refused,
            self.unresolved.len(),
        );
        for (seed, _, which) in self.unknown().take(5) {
            s += &format!("±d differ seed {seed}: {which}\n");
        }
        s
    }
}

/// The tallies of a run.
#[derive(Debug, Default)]
pub(super) struct Report {
    pub placements: u64,
    pub agreed: u64,
    /// Agreed placements where the oracle counted a query shifted.
    pub tied: u64,
    pub mismatches: Vec<(u64, String)>,
    pub refused: BTreeMap<String, Vec<u64>>,
    /// Per cause, where its first refusal happened.
    pub first_at: BTreeMap<String, String>,
    pub unresolved: Vec<(u64, String)>,
    pub tie_scale: Vec<(u64, String)>,
    /// Placements at an operand lobe ([`Outcome::OperandLobe`]).
    pub operand_lobe: Vec<(u64, String)>,
    /// Placements agreeing but for per-face crossing counts at crossed
    /// turned edges ([`Outcome::TurnedEdgeFaces`]).
    pub turned_edge: Vec<(u64, String)>,
    /// Answered placements with a group of faces at a crossed turned
    /// edge, whose crossing counts were checked by their sums.
    pub turned_checked: u64,
    /// Placements whose answers flip only in the oracle, gap below the
    /// probe ([`Outcome::OracleOnlyFlip`]).
    pub oracle_only: Vec<u64>,
    /// Every placement's seed and whether the counting answered (not
    /// refused).
    pub answered: Vec<(u64, bool)>,
    /// The placements' kinds, where the run's family has them: the
    /// report then gives the answered rate per kind.
    pub kinds: Option<fn(u64) -> &'static str>,
    /// The `±d` checks, where the run asked for them.
    pub flip: Option<FlipTally>,
    pub stats: Stats,
    pub seconds: f64,
}

impl Report {
    pub(super) fn add(&mut self, seed: u64, o: Outcome) {
        self.placements += 1;
        self.answered
            .push((seed, !matches!(o, Outcome::Refused(..))));
        match o {
            Outcome::Agreed { ties } => {
                self.agreed += 1;
                self.tied += u64::from(ties > 0);
            }
            Outcome::Mismatch(m) => self.mismatches.push((seed, m)),
            Outcome::Refused(c, at) => {
                let name = cause_name(&c);
                self.first_at.entry(name.clone()).or_insert(at);
                self.refused.entry(name).or_default().push(seed);
            }
            Outcome::Unresolved(u) => self.unresolved.push((seed, u)),
            Outcome::TieScale(m) => self.tie_scale.push((seed, m)),
            Outcome::OperandLobe(m) => self.operand_lobe.push((seed, m)),
            Outcome::TurnedEdgeFaces(m) => self.turned_edge.push((seed, m)),
            Outcome::OracleOnlyFlip => self.oracle_only.push(seed),
        }
    }

    /// [`Report::add`] with the placement's `±d` check.
    pub(super) fn add_checked(&mut self, seed: u64, c: Checked) {
        if let Some(f) = c.flip {
            let known = matches!(c.outcome, Outcome::OperandLobe(_));
            self.flip.get_or_insert_default().add(seed, f, known);
        }
        if c.answers.is_some() && c.turned > 0 {
            self.turned_checked += 1;
        }
        self.add(seed, c.outcome);
    }

    /// The answered rate (placements not refused) per kind, kinds in the
    /// order they first occur.
    pub(super) fn by_kind(&self, kind: fn(u64) -> &'static str) -> String {
        let mut rows: Vec<(&'static str, u64, u64)> = Vec::new();
        for &(seed, answered) in &self.answered {
            let k = kind(seed);
            let i = match rows.iter().position(|r| r.0 == k) {
                Some(i) => i,
                None => {
                    rows.push((k, 0, 0));
                    rows.len() - 1
                }
            };
            rows[i].1 += 1;
            rows[i].2 += u64::from(answered);
        }
        let mut s = String::from("answered by kind:");
        for (k, n, a) in rows {
            let pct = 100.0 * a as f64 / n.max(1) as f64;
            s += &format!(" {k} {a}/{n} ({pct:.1}%);");
        }
        s.pop();
        s + "\n"
    }

    /// The report as text.
    pub(super) fn text(&self) -> String {
        let mut s = String::new();
        let pct = |n: u64| 100.0 * n as f64 / self.placements.max(1) as f64;
        s += &format!(
            "placements {}  agreed {} ({:.2}%)  with oracle ties {}  tie-scale only {}  flip only in the oracle (gap below the probe) {}  per-face counts at crossed turned edges tie-scale {} (sums checked at {})  operand lobes {}  mismatches {}  oracle unresolved {}  time {:.0} s\n",
            self.placements,
            self.agreed,
            pct(self.agreed),
            self.tied,
            self.tie_scale.len(),
            self.oracle_only.len(),
            self.turned_edge.len(),
            self.turned_checked,
            self.operand_lobe.len(),
            self.mismatches.len(),
            self.unresolved.len(),
            self.seconds
        );
        if let Some(kind) = self.kinds {
            s += &self.by_kind(kind);
        }
        if let Some(f) = &self.flip {
            s += &f.text();
        }
        for (c, seeds) in &self.refused {
            let n = seeds.len() as u64;
            let first: Vec<u64> = seeds.iter().take(8).copied().collect();
            s += &format!(
                "refused {c}: {n} ({:.3}%), seeds {first:?}, first at {}\n",
                pct(n),
                self.first_at.get(c).map_or("", |x| x.as_str())
            );
        }
        for (seed, m) in self.mismatches.iter().take(20) {
            s += &format!("MISMATCH seed {seed}: {m}\n");
        }
        for (seed, m) in self.operand_lobe.iter().take(5) {
            s += &format!("operand lobe seed {seed}: {m}\n");
        }
        for (seed, m) in self.tie_scale.iter().take(5) {
            s += &format!("tie-scale seed {seed}: {m}\n");
        }
        for (seed, m) in self.turned_edge.iter().take(5) {
            s += &format!("per-face tie-scale at a crossed turned edge seed {seed}: {m}\n");
        }
        if !self.oracle_only.is_empty() {
            let seeds: Vec<u64> = self.oracle_only.iter().take(12).copied().collect();
            s += &format!("flip only in the oracle, gap below the probe: seeds {seeds:?}\n");
        }
        for (seed, m) in self.unresolved.iter().take(10) {
            s += &format!("unresolved seed {seed}: {m}\n");
        }
        let st = &self.stats;
        for (k, r) in &st.records {
            s += &format!(
                "records {k}: {} decided, top level per record {:?}, exact rationals {}\n",
                r.decided, r.levels, r.rationals
            );
        }
        s += &format!(
            "face counts by top level {:?}; edge pairs' clusters by level {:?}; clear clusters {}, z-order records {}; seeds cleared {} / uncleared {} ({} units); lobe fallbacks {}\n",
            st.counts,
            st.clusters,
            st.clear,
            st.zorder_records,
            st.cleared_seeds,
            st.uncleared_seeds,
            st.seed_units,
            st.lobes
        );
        s += &format!(
            "edge pairs by the shared-curve rule {}; every edge pair's refusals by cause {:?}\n",
            st.shared, st.pair_causes
        );
        s += &format!(
            "seconds: records {:.0}, face counts {:.0} (corners {:.0}), clusters {:.0}\n",
            st.time[0], st.time[1], st.corners, st.time[2]
        );
        s
    }
}

/// A cause's name: its kind and the kind of the failure it carries
/// (`Clusters(OutOfBudget)`), without keys, boxes or levels, so that one
/// kind is one row of a report.
pub(super) fn cause_name(c: &Cause) -> String {
    match c {
        Cause::Record { kind, fail } => format!("Record({kind}: {})", head(fail)),
        Cause::OutOfBudget(s) => format!("OutOfBudget({})", head(s)),
        Cause::Side(s) => format!("Side({})", head(s)),
        Cause::Height => "Height".into(),
        Cause::Count(s) => format!("Count({})", nested(s)),
        Cause::Clusters(s) => format!("Clusters({})", head(s)),
        Cause::ZOrder => "ZOrder".into(),
        Cause::Shared(s) => format!("Shared({})", head(s)),
        Cause::Seeds(s) => format!("Seeds({})", head(s)),
        Cause::Signal(s) => format!("Signal({})", head(s)),
        Cause::Other(s) => format!("Other({})", head(s)),
    }
}

/// The text of a failure's debug form before its first `(` or `{`: its
/// kind (a variant's name, after a word naming the step where there is
/// one).
fn head(s: &str) -> &str {
    s.split(['(', '{']).next().unwrap_or("").trim()
}

/// A failure's kind and, where it wraps another failure, that one's
/// kind: `Piece(Ribbon)` of `Piece(Ribbon { coedge: .. })`.
fn nested(s: &str) -> String {
    let outer = head(s);
    match s[outer.len()..].trim_start().strip_prefix('(') {
        Some(rest) => format!("{outer}({})", head(rest).trim_end_matches(')')),
        None => outer.to_string(),
    }
}

/// A refusal limit: at most `per_mille / 1000` of a run's placements
/// refused, asserted once the run has `min` placements (fewer can't
/// measure the rate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Limit {
    pub per_mille: u64,
    pub min: u64,
}

impl Limit {
    /// The limit as a percentage.
    pub(super) fn percent(self) -> f64 {
        self.per_mille as f64 / 10.0
    }
}

impl Report {
    /// The placements the counting refused.
    pub(super) fn refusals(&self) -> u64 {
        self.refused.values().map(|s| s.len() as u64).sum()
    }

    /// The refused placements' rate against `limit`, as a line of the
    /// report: `refused 9/100 (9.0%), limit 10% within`.
    pub(super) fn against(&self, limit: Limit) -> String {
        let (k, n) = (self.refusals(), self.placements);
        let state = if n < limit.min {
            format!("not asserted below {} placements", limit.min)
        } else if self.over(limit) {
            "OVER".into()
        } else {
            "within".into()
        };
        format!(
            "refused {k}/{n} ({:.1}%), limit {}% {state}",
            100.0 * k as f64 / n.max(1) as f64,
            limit.percent()
        )
    }

    /// Whether the refusals exceed `limit` (at any number of
    /// placements): exactly, `refused / placements > per_mille / 1000`.
    fn over(&self, limit: Limit) -> bool {
        u128::from(self.refusals()) * 1000
            > u128::from(limit.per_mille) * u128::from(self.placements)
    }

    /// `Err` naming the run `name`, its rate, the limit and the refusals
    /// by cause if the run has at least `limit.min` placements and its
    /// refusals exceed `limit`.
    pub(super) fn within(&self, name: &str, limit: Limit) -> Result<(), String> {
        if self.placements < limit.min || !self.over(limit) {
            return Ok(());
        }
        let causes: Vec<String> = self
            .refused
            .iter()
            .map(|(c, seeds)| format!("{c} {}", seeds.len()))
            .collect();
        Err(format!(
            "{name}: {}; causes {}",
            self.against(limit),
            causes.join(", ")
        ))
    }
}

/// Checks the placements `make(i)`, `i` in `0..n`, on
/// `threads` threads, with the `±d` check if `flip`; outcomes are
/// tallied in `i`'s order.
pub(super) fn run(
    n: u64,
    threads: usize,
    cfg: &Config,
    flip: bool,
    make: &(dyn Fn(u64) -> Placement + Sync),
) -> Report {
    run_counted(n, threads, cfg, flip, counting, make)
}

/// [`run`] with the placements counted by `count`.
#[allow(
    clippy::disallowed_methods,
    reason = "times its steps for the report: no result reads the clock"
)]
pub(super) fn run_counted(
    n: u64,
    threads: usize,
    cfg: &Config,
    flip: bool,
    count: Counting,
    make: &(dyn Fn(u64) -> Placement + Sync),
) -> Report {
    let start = std::time::Instant::now();
    let done = par(n, threads, &|i| {
        let mut stats = Stats::default();
        let c = check_counted(&make(i), cfg, &mut stats, flip, count);
        (i, c, stats)
    });
    let mut r = Report::default();
    if flip {
        r.flip = Some(FlipTally::default());
    }
    for (i, c, st) in done {
        r.stats.add(&st);
        r.add_checked(i, c);
    }
    r.seconds = start.elapsed().as_secs_f64();
    r
}

/// `f(i)` for `i` in `0..n` on `threads` threads, in `i`'s order.
pub(super) fn par<T: Send>(n: u64, threads: usize, f: &(dyn Fn(u64) -> T + Sync)) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(u64, T)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed) as u64;
                    if i >= n {
                        break;
                    }
                    let x = f(i);
                    done.lock().expect("no panic").push((i, x));
                }
            });
        }
    });
    let mut done = done.into_inner().expect("no panic");
    done.sort_by_key(|x| x.0);
    done.into_iter().map(|x| x.1).collect()
}
