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

/// Whether two of the oracle's answers are the same (their ties aside).
fn same(x: &Truth, y: &Truth) -> bool {
    x.winding == y.winding && x.x12 == y.x12 && x.x21 == y.x21
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
    /// The counting's answer differs from the oracle's (with its tie
    /// shift, or at `2^-28·d` or `−2^-28·d`) only at vertices whose
    /// column meets a layer decided by the height of an edge of the face,
    /// at the vertex's foot on it, and on the edges from them: in the
    /// edge's band the counting takes the edge's height as the face's,
    /// which the true face, folding through the vertex's column there,
    /// needn't follow. Known, and counted apart from mismatches.
    EdgeHeight(String),
}

/// Where the counting's answers differ from the oracle's, if anywhere.
pub(super) fn compare(c: &Answers, t: &Truth) -> Option<String> {
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
    for (name, cx, tx) in [("x12", &c.x12, &t.x12), ("x21", &c.x21, &t.x21)] {
        for (e, (cr, tr)) in cx.iter().zip(tx).enumerate() {
            for (f, (x, y)) in cr.iter().zip(tr).enumerate() {
                if x != y {
                    out.push(format!("{name} e{e} f{f}: counted {x}, oracle {y}"));
                }
            }
        }
    }
    (!out.is_empty()).then(|| out.join("; "))
}

/// Whether every difference between the counting's answers and the
/// oracle's lies where a layer was decided by an edge's height
/// ([`Answers::edge_height`]): a winding or a column at such a vertex,
/// and a crossing count of an edge from such a vertex with that face.
/// Windings are carried from a seed along the edges, but a column and
/// the crossing counts of the edges from its vertex move together, so
/// the windings at the other vertices keep the oracle's.
pub(super) fn by_edge_height(c: &Answers, t: &Truth, a: &Solid, b: &Solid) -> bool {
    let eh = &c.edge_height;
    for k in 0..2 {
        for (v, o) in t.winding[k].iter().enumerate() {
            let differs = c.winding[k][v] != *o || c.column[k][v] != *o;
            if differs && !eh[k][v].iter().any(|&x| x) {
                return false;
            }
        }
    }
    for (k, s, cx, tx) in [(0, a, &c.x12, &t.x12), (1, b, &c.x21, &t.x21)] {
        for (e, (cr, tr)) in s.edges().into_iter().zip(cx.iter().zip(tx)) {
            for (f, (x, y)) in cr.iter().zip(tr).enumerate() {
                if x != y && !s.brep.ends(e).iter().any(|v| eh[k][v.index()][f]) {
                    return false;
                }
            }
        }
    }
    true
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
}

/// A placement's check: its outcome, the counting's answers where it
/// gave them, and the `±d` check where asked for and answered.
#[derive(Clone, Debug)]
pub(super) struct Checked {
    pub outcome: Outcome,
    pub answers: Option<Answers>,
    pub flip: Option<Flip>,
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
    let answers = match count(&a, &b, p.d, cfg, stats) {
        Ok(x) => x,
        Err(r) => {
            return Checked {
                outcome: Outcome::Refused(r.cause, r.at),
                answers: None,
                flip: None,
            };
        }
    };
    let (fa, fb) = (oracle_faces(&a), oracle_faces(&b));
    let near = OnceCell::new();
    let near = || near.get_or_init(|| truths_near(&a, &b, p.d, &fa, &fb));
    let outcome = judge(p, &a, &b, &answers, &fa, &fb, &near);
    let flip = (flip && !matches!(outcome, Outcome::Unresolved(_))).then(|| {
        // Its own tallies: the run's are the base counts'.
        let minus = count(&a, &b, -p.d, cfg, &mut Stats::default());
        match (minus, near()) {
            (Err(r), _) => Flip::Refused(r.cause),
            (_, Err(e)) => Flip::Unresolved(e.clone()),
            (Ok(m), Ok([up, down])) => Flip::Checked {
                counted: answers.winding != m.winding
                    || answers.column != m.column
                    || answers.x12 != m.x12
                    || answers.x21 != m.x21,
                truth: !same(up, down),
                sides: [compare(&answers, up).is_none(), compare(&m, down).is_none()],
            },
        }
    });
    Checked {
        outcome,
        answers: Some(answers),
        flip,
    }
}

/// The oracle's verdict on the counting's answers; `near` gives the
/// oracle's answers at `±2^-28·d` ([`truths_near`]).
fn judge<'n>(
    p: &Placement,
    a: &Solid,
    b: &Solid,
    answers: &Answers,
    fa: &[ExactFace],
    fb: &[ExactFace],
    near: &dyn Fn() -> &'n Result<[Truth; 2], String>,
) -> Outcome {
    let t = match truth(a, b, p.d, fa, fb) {
        Err(e) => return Outcome::Unresolved(e),
        Ok(t) => t,
    };
    let Some(m) = compare(answers, &t) else {
        return Outcome::Agreed { ties: t.ties };
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
    if compare(answers, up).is_none() || compare(answers, down).is_none() {
        let tie = if t.ties > 0 {
            "(oracle tie)"
        } else {
            "(no oracle tie)"
        };
        Outcome::TieScale(format!("{tie} {m}"))
    } else if [&t, up, down]
        .iter()
        .any(|t| by_edge_height(answers, t, a, b))
    {
        Outcome::EdgeHeight(m)
    } else {
        let near = if same(up, down) {
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
    /// [`Outcome::EdgeHeight`] (known), and which flipped.
    pub differ: Vec<(u64, bool, String)>,
    /// The counting refused with `−d`, by cause.
    pub refused: BTreeMap<String, u64>,
    pub unresolved: Vec<(u64, String)>,
}

impl FlipTally {
    fn add(&mut self, seed: u64, f: Flip, known: bool) {
        match f {
            Flip::Refused(c) => *self.refused.entry(cause_name(&c)).or_default() += 1,
            Flip::Unresolved(u) => self.unresolved.push((seed, u)),
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

    /// The disagreements not known from the edge-height rule.
    pub(super) fn unknown(&self) -> impl Iterator<Item = &(u64, bool, String)> {
        self.differ.iter().filter(|x| !x.1)
    }

    fn text(&self) -> String {
        let known = self.differ.iter().filter(|x| x.1).count();
        let seeds: Vec<u64> = self.unknown().take(12).map(|x| x.0).collect();
        let mut s = format!(
            "±d check: counted both ways {}; answers flip with d: counted {}, oracle at ±2^-28·d {}; agree {}, differ {} (edge-height known {known}, others at seeds {seeds:?}); both sides equal the oracle's {}; refused with −d {:?}; oracle unresolved {}\n",
            self.both,
            self.counted,
            self.truth,
            self.both - self.differ.len() as u64,
            self.differ.len(),
            self.sides,
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
    /// Placements differing from the oracle by the edge-height rule only
    /// ([`Outcome::EdgeHeight`]).
    pub edge_height: Vec<(u64, String)>,
    /// The `±d` checks, where the run asked for them.
    pub flip: Option<FlipTally>,
    pub stats: Stats,
    pub seconds: f64,
}

impl Report {
    pub(super) fn add(&mut self, seed: u64, o: Outcome) {
        self.placements += 1;
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
            Outcome::EdgeHeight(m) => self.edge_height.push((seed, m)),
        }
    }

    /// [`Report::add`] with the placement's `±d` check.
    pub(super) fn add_checked(&mut self, seed: u64, c: Checked) {
        if let Some(f) = c.flip {
            let known = matches!(c.outcome, Outcome::EdgeHeight(_));
            self.flip.get_or_insert_default().add(seed, f, known);
        }
        self.add(seed, c.outcome);
    }

    /// The report as text.
    pub(super) fn text(&self) -> String {
        let mut s = String::new();
        let pct = |n: u64| 100.0 * n as f64 / self.placements.max(1) as f64;
        s += &format!(
            "placements {}  agreed {} ({:.2}%)  with oracle ties {}  tie-scale only {}  edge-height known {}  mismatches {}  oracle unresolved {}  time {:.0} s\n",
            self.placements,
            self.agreed,
            pct(self.agreed),
            self.tied,
            self.tie_scale.len(),
            self.edge_height.len(),
            self.mismatches.len(),
            self.unresolved.len(),
            self.seconds
        );
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
        for (seed, m) in self.edge_height.iter().take(5) {
            s += &format!("edge-height known seed {seed}: {m}\n");
        }
        for (seed, m) in self.tie_scale.iter().take(5) {
            s += &format!("tie-scale seed {seed}: {m}\n");
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
            "face counts by top level {:?}; edge pairs' clusters by level {:?}; clear clusters {}, z-order stand-ins {}; seeds cleared {} / uncleared {}\n",
            st.counts, st.clusters, st.clear, st.standins, st.cleared_seeds, st.uncleared_seeds
        );
        s += &format!(
            "edge pairs on one curve {}; every edge pair's refusals by cause {:?}\n",
            st.shared, st.pair_causes
        );
        s += &format!(
            "seconds: records {:.0}, face counts {:.0} (corners {:.0}), clusters {:.0}\n",
            st.time[0], st.time[1], st.corners, st.time[2]
        );
        s
    }
}

/// A short name for a cause, without its keys.
fn cause_name(c: &Cause) -> String {
    match c {
        Cause::Record { kind, fail } => {
            let f: String = fail.split(['{', '(']).next().unwrap_or("").into();
            format!("Record({kind}: {f})")
        }
        Cause::Clusters(s) => {
            let k = s.split('(').next().unwrap_or("");
            let why = s.rsplit(", ").next().unwrap_or("").trim_end_matches(')');
            format!("Clusters({k}, {why})")
        }
        c => {
            let s = format!("{c:?}");
            s.chars().take(60).collect()
        }
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
