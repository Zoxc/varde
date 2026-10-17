//! The runs: placements of two solids, each counted ([`World::count`])
//! and checked against the ray oracle, on several threads; the tallies
//! of agreements, refusals by cause, the oracle's ties, and the
//! counting's precision levels.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use glam::DVec3;
use varde_testkit::ray_oracle::{Chain, Coedge, ExactFace, Face, Shift, UvChain};
use varde_testkit::rng::Rng;

use super::solids::{Make, Shape, Solid};
use super::world::{Answers, Cause, Config, Stats, World};
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
    /// The counting's answer differs from the oracle's at a placement
    /// within the faces' rounding misfit of a tie, and equals the
    /// oracle's with `A` moved by `2^-28·d` or `−2^-28·d` (the counting's
    /// flat records and the oracle's tiles differ below ~1e-10 mm).
    TieScale(String),
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

/// A placement's solids, its counting and its check.
pub(super) fn check(p: &Placement, cfg: &Config, stats: &mut Stats) -> Outcome {
    check_answers(p, cfg, stats).0
}

/// [`check`], with the counting's answers where it gave them.
pub(super) fn check_answers(
    p: &Placement,
    cfg: &Config,
    stats: &mut Stats,
) -> (Outcome, Option<Answers>) {
    let (a, b) = (p.a.build(&p.make_a), p.b.build(&p.make_b));
    let w = World::new(&a, &b, p.d);
    let answers = match w.count(cfg, stats) {
        Ok(x) => x,
        Err(r) => return (Outcome::Refused(r.cause, r.at), None),
    };
    let o = judge(p, &a, &b, &answers);
    (o, Some(answers))
}

/// The oracle's verdict on the counting's answers.
fn judge(p: &Placement, a: &Solid, b: &Solid, answers: &Answers) -> Outcome {
    let (fa, fb) = (oracle_faces(a), oracle_faces(b));
    match truth(a, b, p.d, &fa, &fb) {
        Err(e) => Outcome::Unresolved(e),
        Ok(t) => match compare(answers, &t) {
            None => Outcome::Agreed { ties: t.ties },
            Some(m) => {
                // Within the faces' rounding misfit (a plane tile's patch
                // passes within ~1e-10 mm of its key's plane and its
                // vertices, not through them) the counting surface the
                // oracle casts against and the records' flat geometry
                // (vertices, key planes) differ, and the oracle's tie
                // shift `2^-512·d` lands inside that misfit. Such a
                // placement is within the misfit of a tie when moving `A`
                // by `±2^-28·d` (about 4e-9 mm: past the misfit and the
                // records' error target `2^-30`, within every band)
                // changes the oracle's answers: the counting must then
                // agree with one of the two.
                let at = |sign: f64| {
                    truth_at(a, b, &Shift::always((p.d * sign).to_array(), -28), &fa, &fb)
                };
                match (at(1.0), at(-1.0)) {
                    (Err(e), _) | (_, Err(e)) => Outcome::Unresolved(e),
                    (Ok(up), Ok(down)) => {
                        let unstable =
                            up.winding != down.winding || up.x12 != down.x12 || up.x21 != down.x21;
                        let matches =
                            compare(answers, &up).is_none() || compare(answers, &down).is_none();
                        if unstable && matches {
                            Outcome::TieScale(format!(
                                "{} {m}",
                                if t.ties > 0 {
                                    "(oracle tie)"
                                } else {
                                    "(no oracle tie)"
                                }
                            ))
                        } else {
                            Outcome::Mismatch(format!(
                                "{}{m}",
                                if unstable {
                                    "(within 2^-28 of a tie) "
                                } else {
                                    ""
                                }
                            ))
                        }
                    }
                }
            }
        },
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
        }
    }

    /// The report as text.
    pub(super) fn text(&self) -> String {
        let mut s = String::new();
        let pct = |n: u64| 100.0 * n as f64 / self.placements.max(1) as f64;
        s += &format!(
            "placements {}  agreed {} ({:.2}%)  with oracle ties {}  tie-scale only {}  mismatches {}  oracle unresolved {}  time {:.0} s\n",
            self.placements,
            self.agreed,
            pct(self.agreed),
            self.tied,
            self.tie_scale.len(),
            self.mismatches.len(),
            self.unresolved.len(),
            self.seconds
        );
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

/// Runs `check` on the placements `make(i)`, `i` in `0..n`, on
/// `threads` threads; outcomes are tallied in `i`'s order.
pub(super) fn run(
    n: u64,
    threads: usize,
    cfg: &Config,
    make: &(dyn Fn(u64) -> Placement + Sync),
) -> Report {
    let start = std::time::Instant::now();
    let done = par(n, threads, &|i| {
        let mut stats = Stats::default();
        let o = check(&make(i), cfg, &mut stats);
        (i, o, stats)
    });
    let mut r = Report::default();
    for (i, o, st) in done {
        r.stats.add(&st);
        r.add(i, o);
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
