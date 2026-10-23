//! Coincident arcs, the vertices records place on edges, and zero-length
//! edges in a face's arrangement: all by record, never by distance.
//!
//! **Input** ([`FaceInput`]). The face's vertices, each placed by its
//! record ([`Place`]); its **lines**, the curves other edges may run
//! along: the operand edges bounding the face, and new arcs other arcs
//! coincide with, each with its p-curve on the face ([`Line`]); and its
//! new **arcs**, each between two vertices, either transversal (a span
//! of its own) or along a chain of lines ([`Arc::along`]), as the records
//! that made it say: two surfaces of which one is the face's across the
//! line, an edge-on record of the line's coedge and the arc's other
//! surface, a shared curve, or two coincident pairs. Which arcs coincide
//! with what is the caller's, from those records; nothing here compares
//! positions to find it out.
//!
//! **Points.** A vertex lies at a point named combinatorially: its own
//! (an operand vertex, a record inside the face), another vertex's
//! ([`Place::At`]: another record at one operand vertex or one cut
//! point), or a parameter of a line ([`Place::On`], [`Place::Pin`]). Two
//! vertices are at one point only if their places name one point; a
//! point is where its name puts it: the given UV, or the line's
//! p-curve's canonical point at the parameter ([`PCurve::point_at`]), so
//! every vertex there has the same bits. Distinct vertices at one point
//! are a vertex contact ([`Point::verts`]); one point is one vertex of
//! the planar arrangement.
//!
//! **Splits** (which records split a line). A line is split at the
//! parameter of each vertex placed [`Place::On`] it (the record lies on
//! the line in the moved operand: the vertex **is** the line's vertex
//! there) and of each [`Place::Pin`] (the record is at that point of the
//! line without lying on it: a vertex of another member there). A pin
//! splits the line at a pin vertex of the line's own ([`Coincident::pins`]),
//! joined to the record's vertex by a zero-length edge; where an `On`
//! vertex is the line's vertex at that parameter, the pin joins that one
//! instead. A line's pieces run between consecutive splits from its stored
//! start.
//!
//! **Coincident arcs.** An arc along a chain of lines is a piece of
//! them: it is split at every vertex of the chain strictly inside its
//! span (the chain's own joins, the projected vertices, and the lines'
//! splits there), and each of its pieces reuses the p-curve of the line
//! piece it lies on, over that piece's parameters, bit for bit. Its ends
//! must be at points of the chain: its first end at a split or the start
//! of the first line (as the chain runs it), its last at a split or the
//! end of the last, with the span running forward along each line. The
//! pieces lying on one line piece, the line's own and the arcs', form a
//! **bundle** ([`Bundle`]): one edge of the planar arrangement whose
//! winding step is the sum of its members' (a contact of edges where it
//! has more than one; the zero-area regions between them are not built
//! here). A transversal arc is a bundle of its own.
//!
//! **Zero-length edges** ([`Zero`]). An arc whose two ends are at one
//! point (records of one operand vertex, or of one cut point) is a
//! zero-length edge there, whatever it runs along; so is each pin's join.
//! They are no edges of the planar arrangement. A point that only
//! zero-length edges reach ([`Point::contracted`]: a whole loop of them,
//! the hole a point contact cuts) is contracted to that point: the face
//! stays whole there.
//!
//! **Arrangement.** The planar arrangement ([`arrange2d`]) is built on the
//! points (by point id) and the bundles: a line piece's bundle runs along
//! the chords of its p-curve between the knots strictly inside it (a
//! bend there, an arrangement vertex of its own after the points), a
//! transversal one straight. Coincident spans without a record that
//! links them, and bitwise-equal points with different names, reach it
//! as such and are refused there, never merged.
//!
//! **Order.** Points are numbered by their naming vertex (a pin vertex
//! after the face's vertices, pins by line, then by parameter); pieces
//! line by line from the stored start, then the coincident arcs' arc by
//! arc along the arc, then the transversal arcs'; zero-length edges arc
//! by arc, then pin by pin; bundles line piece by line piece, then
//! transversal arc by arc. The numbering follows the
//! input's; nothing decided here reads it.
//!
//! **Work.** One pass on the operation's budget, one item: a unit per
//! vertex, line, arc, piece, pin's join, bend, arrangement span and step
//! looking up an arc's end on a line; then the arrangement's passes.

use std::collections::BTreeMap;

use crate::geom::pcurve::PCurve;
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;
use crate::topo::arrange2d::{self, ArrangeError, Arrangement, Edge, NONE};

/// Where a vertex of the face lies, as its record places it (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// At this point of the face's UV: an operand vertex, or a record
    /// inside the face.
    Uv([f64; 2]),
    /// At this other vertex's point, a distinct vertex there; the other
    /// vertex is not placed `At` itself.
    At(u32),
    /// The vertex of this line at its parameter `s`, strictly inside the
    /// line's range: the record lies on the line and splits it there.
    On {
        /// The line.
        line: u32,
        /// The parameter.
        s: f64,
    },
    /// At this line's point at its parameter `s`, strictly inside its
    /// range, without lying on it: the line is split there at a vertex of
    /// its own, joined to this one by a zero-length edge.
    Pin {
        /// The line.
        line: u32,
        /// The parameter.
        s: f64,
    },
}

/// A line of the face (module docs): an operand edge bounding it, or a
/// new arc others coincide with.
#[derive(Clone, Copy, Debug)]
pub struct Line<'a> {
    /// Its vertices at its p-curve's first and last knot.
    pub ends: [u32; 2],
    /// Its p-curve on the face, from `ends[0]` to `ends[1]`.
    pub pcurve: &'a PCurve,
    /// The winding number left of it minus the one right of it, along its
    /// p-curve.
    pub wind: i32,
}

/// A line of an arc's chain, and the way the chain runs along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Along {
    /// The line.
    pub line: u32,
    /// Whether the chain runs along the line's p-curve (from `ends[0]`).
    pub forward: bool,
}

/// A new arc on the face (module docs).
#[derive(Clone, Copy, Debug)]
pub struct Arc<'a> {
    /// Its start and end vertex.
    pub ends: [u32; 2],
    /// The chain of lines it runs along, in its direction, each line's
    /// last point the next one's first; empty for a transversal arc.
    pub along: &'a [Along],
    /// The winding number left of it minus the one right of it, in its
    /// direction.
    pub wind: i32,
}

/// A face's vertices, lines and arcs (module docs).
#[derive(Clone, Copy, Debug)]
pub struct FaceInput<'a> {
    /// The vertices, by index.
    pub verts: &'a [Place],
    /// The lines.
    pub lines: &'a [Line<'a>],
    /// The arcs.
    pub arcs: &'a [Arc<'a>],
}

/// A line's pin vertex (module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PinVert {
    /// The line.
    pub line: u32,
    /// The parameter on it.
    pub s: f64,
}

/// A point of the face (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    /// Where it lies in the face's UV.
    pub uv: [f64; 2],
    /// The vertices there that an edge uses, ascending: a vertex contact
    /// where there is more than one.
    pub verts: Vec<u32>,
    /// Whether only zero-length edges reach it, so it is contracted.
    pub contracted: bool,
}

/// What a piece is a piece of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceOf {
    /// The `part`-th piece of this line from its stored start.
    Line {
        /// The line.
        line: u32,
        /// The piece's index.
        part: u32,
    },
    /// The `part`-th piece of this arc along it.
    Arc {
        /// The arc.
        arc: u32,
        /// The piece's index.
        part: u32,
    },
}

/// An edge of positive length: a piece of a line or of an arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    /// What it is a piece of.
    pub of: PieceOf,
    /// Its start and end vertex.
    pub ends: [u32; 2],
    /// The line whose p-curve it reuses and the parameters there at its
    /// start and its end; `None` for a transversal arc.
    pub reuses: Option<(u32, [f64; 2])>,
    /// Its bundle.
    pub bundle: u32,
    /// Whether it runs the bundle's way.
    pub forward: bool,
}

/// What a zero-length edge comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZeroOf {
    /// This arc, its ends at one point.
    Arc(u32),
    /// The join of this pinned vertex to its line's vertex there.
    Pin(u32),
}

/// A zero-length edge (module docs): constant at its point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zero {
    /// What it comes from.
    pub of: ZeroOf,
    /// Its start and end vertex (for a pin's join: the line's vertex,
    /// then the pinned one).
    pub ends: [u32; 2],
    /// Its point.
    pub point: u32,
}

/// The pieces lying on one line piece, or a transversal arc: one edge of
/// the planar arrangement (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Bundle {
    /// Its pieces: the line's piece first, then the arcs' in arc order.
    pub members: Vec<u32>,
    /// The points at its start and end.
    pub ends: [u32; 2],
    /// The sum of its members' winding steps, in its direction.
    pub wind: i32,
}

/// A face's coincident arcs, splits and zero-length edges realised, and
/// its planar arrangement on them (module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Coincident {
    /// Per vertex, the face's then the pin vertices: its point.
    pub point_of: Vec<u32>,
    /// The lines' pin vertices; pin `k` is vertex `verts.len() + k`.
    pub pins: Vec<PinVert>,
    /// The points.
    pub points: Vec<Point>,
    /// The edges of positive length.
    pub pieces: Vec<Piece>,
    /// The zero-length edges.
    pub zeros: Vec<Zero>,
    /// The bundles.
    pub bundles: Vec<Bundle>,
    /// Per edge of the arrangement, its bundle.
    pub span_bundle: Vec<u32>,
    /// The planar arrangement: its vertices the points, then the bends.
    pub arrangement: Arrangement,
}

/// Why the face can't be realised (module docs).
#[derive(Clone, Debug, PartialEq)]
pub enum CoincidentError {
    /// More vertices, lines, arcs or pieces than `u32` ids hold.
    TooMany,
    /// This vertex's place names a vertex or line not there, a vertex
    /// placed `At` itself, a parameter not strictly inside its line's
    /// range, or a coordinate that isn't finite.
    Place(u32),
    /// This line names a vertex not there, its ends are at one point, or
    /// its p-curve doesn't end at its ends' points bit for bit.
    Line(u32),
    /// This vertex is placed `On` its line at a parameter another vertex
    /// already is.
    Split(u32),
    /// This arc names a vertex or line not there, its ends are one
    /// vertex, its chain doesn't join up, or its ends don't bound a
    /// forward span of the chain.
    Arc(u32),
    /// This bundle's winding step leaves `i32`.
    Wind(u32),
    /// The planar arrangement refused (its edge ids read as bundles, its
    /// vertex ids as points, then bends).
    Arrange(ArrangeError),
    /// The budget ran out, or a fixed limit was reached
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

/// The face `face` realised and arranged (module docs), charged to
/// `work`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the boolean's face arrangements call it as they land"
    )
)]
pub(crate) fn realise(
    face: &FaceInput<'_>,
    work: &mut Work,
) -> Result<Coincident, CoincidentError> {
    let built = work
        .pass(
            Pass {
                stage: "coincident arcs",
                rung: 0,
            },
            &[()],
            |_, ledger| build(face, ledger),
        )
        .map_err(|halt| match halt {
            Halt::Failed { error, .. } => error,
            Halt::OutOfBudget(e) => CoincidentError::OutOfBudget(e),
        })?
        .swap_remove(0);
    let arrangement =
        arrange2d::arrange(&built.nodes, &built.spans, work).map_err(|e| match e {
            ArrangeError::OutOfBudget(k) => CoincidentError::OutOfBudget(k),
            e => CoincidentError::Arrange(as_bundles(e, &built.span_bundle)),
        })?;
    Ok(Coincident {
        point_of: built.point_of,
        pins: built.pins,
        points: built.points,
        pieces: built.pieces,
        zeros: built.zeros,
        bundles: built.bundles,
        span_bundle: built.span_bundle,
        arrangement,
    })
}

/// The arrangement's error with its edge ids read as bundles.
fn as_bundles(e: ArrangeError, span_bundle: &[u32]) -> ArrangeError {
    let b = |e: u32| span_bundle.get(e as usize).copied().unwrap_or(NONE);
    match e {
        ArrangeError::Index(e) => ArrangeError::Index(b(e)),
        ArrangeError::SelfLoop(e) => ArrangeError::SelfLoop(b(e)),
        ArrangeError::ZeroLength(e) => ArrangeError::ZeroLength(b(e)),
        ArrangeError::Winding(e) => ArrangeError::Winding(b(e)),
        ArrangeError::Coincident { vertex, edges } => {
            let (x, y) = (b(edges[0]), b(edges[1]));
            ArrangeError::Coincident {
                vertex,
                edges: if x <= y { [x, y] } else { [y, x] },
            }
        }
        ArrangeError::Overlap { edges } => {
            let (x, y) = (b(edges[0]), b(edges[1]));
            ArrangeError::Overlap {
                edges: if x <= y { [x, y] } else { [y, x] },
            }
        }
        e => e,
    }
}

type Res<T> = Result<T, Stop<CoincidentError>>;

fn fail<T>(e: CoincidentError) -> Res<T> {
    Err(Stop::Failed(e))
}

/// The id of the next entry of a column `len` long, or `TooMany`.
fn next_id(len: usize) -> Res<u32> {
    u32::try_from(len)
        .ok()
        .filter(|&i| i != NONE)
        .ok_or(Stop::Failed(CoincidentError::TooMany))
}

fn units(ledger: &mut Ledger<'_>, n: u64) -> Res<()> {
    ledger.charge(Step::Work, n).map_err(Stop::from)
}

/// A parameter as a key: `−0` read as `0`, so equal values are one key.
fn bits(s: f64) -> u64 {
    if s == 0.0 { 0 } else { s.to_bits() }
}

/// Whether two UV points are the same bits.
fn same_bits(p: [f64; 2], q: [f64; 2]) -> bool {
    p[0].to_bits() == q[0].to_bits() && p[1].to_bits() == q[1].to_bits()
}

/// A split of a line: its parameter and the line's vertex there.
#[derive(Clone, Copy, Debug)]
struct Cut {
    s: f64,
    vert: u32,
}

/// The realisation before arranging ([`Coincident`]'s fields), and the
/// arrangement's input.
struct Built {
    point_of: Vec<u32>,
    pins: Vec<PinVert>,
    points: Vec<Point>,
    pieces: Vec<Piece>,
    zeros: Vec<Zero>,
    bundles: Vec<Bundle>,
    span_bundle: Vec<u32>,
    nodes: Vec<[f64; 2]>,
    spans: Vec<Edge>,
}

/// The realisation (module docs), on one ledger.
fn build(face: &FaceInput<'_>, ledger: &mut Ledger<'_>) -> Res<Built> {
    let (verts, lines, arcs) = (face.verts, face.lines, face.arcs);
    let n = verts.len();
    if n >= NONE as usize || lines.len() >= NONE as usize || arcs.len() >= NONE as usize {
        return fail(CoincidentError::TooMany);
    }
    let range = |l: &Line<'_>| {
        let k = l.pcurve.knots();
        (k[0], k[k.len() - 1])
    };

    // Places.
    for (v, p) in verts.iter().enumerate() {
        units(ledger, 1)?;
        let ok = match *p {
            Place::Uv(uv) => uv.iter().all(|x| x.is_finite()),
            Place::At(t) => {
                (t as usize) < n && t as usize != v && !matches!(verts[t as usize], Place::At(_))
            }
            Place::On { line, s } | Place::Pin { line, s } => {
                lines.get(line as usize).is_some_and(|l| {
                    let (a, b) = range(l);
                    a < s && s < b
                })
            }
        };
        if !ok {
            return fail(CoincidentError::Place(v as u32));
        }
    }

    // The lines' vertices at their splits: `On` vertices, then the pin
    // vertices where no `On` vertex is.
    let mut on: BTreeMap<(u32, u64), u32> = BTreeMap::new();
    for (v, p) in verts.iter().enumerate() {
        if let Place::On { line, s } = *p
            && on.insert((line, bits(s)), v as u32).is_some()
        {
            return fail(CoincidentError::Split(v as u32));
        }
    }
    let mut pin_at: BTreeMap<(u32, u64), f64> = BTreeMap::new();
    for p in verts {
        if let Place::Pin { line, s } = *p
            && !on.contains_key(&(line, bits(s)))
        {
            pin_at.insert((line, bits(s)), s);
        }
    }
    if n + pin_at.len() >= NONE as usize {
        return fail(CoincidentError::TooMany);
    }
    let mut pins: Vec<PinVert> = pin_at
        .values()
        .zip(pin_at.keys())
        .map(|(&s, &(line, _))| PinVert { line, s })
        .collect();
    pins.sort_by(|x, y| x.line.cmp(&y.line).then(x.s.total_cmp(&y.s)));
    let mut line_vert = on.clone();
    for (k, p) in pins.iter().enumerate() {
        line_vert.insert((p.line, bits(p.s)), (n + k) as u32);
    }
    let all = n + pins.len();
    units(ledger, pins.len() as u64)?;

    // Each vertex's naming vertex, then the points in that order.
    let name = |v: usize| -> u32 {
        let v = match verts.get(v) {
            Some(Place::At(t)) => *t as usize,
            _ => v,
        };
        match verts.get(v) {
            Some(Place::Pin { line, s }) => line_vert[&(*line, bits(*s))],
            _ => v as u32,
        }
    };
    let mut point_id = vec![NONE; all];
    for v in 0..all {
        point_id[name(v) as usize] = 0;
    }
    let mut points = Vec::new();
    for v in 0..all {
        if point_id[v] == NONE {
            continue;
        }
        point_id[v] = points.len() as u32;
        let uv = if v < n {
            match verts[v] {
                Place::Uv(uv) => Some(uv),
                Place::On { line, s } => lines[line as usize].pcurve.point_at(s),
                // Not a naming vertex.
                Place::At(_) | Place::Pin { .. } => None,
            }
        } else {
            let p = pins[v - n];
            lines[p.line as usize].pcurve.point_at(p.s)
        };
        let Some(uv) = uv.filter(|uv| uv.iter().all(|x| x.is_finite())) else {
            return fail(CoincidentError::Place(v as u32));
        };
        points.push(Point {
            uv,
            verts: Vec::new(),
            contracted: false,
        });
    }
    let point_of: Vec<u32> = (0..all).map(|v| point_id[name(v) as usize]).collect();

    // The lines: their ends, their splits, their pieces and bundles.
    let mut pieces: Vec<Piece> = Vec::new();
    let mut bundles: Vec<Bundle> = Vec::new();
    let mut cuts: Vec<Vec<Cut>> = Vec::with_capacity(lines.len());
    // Per line, the bundle of its first piece.
    let mut first_bundle: Vec<u32> = Vec::with_capacity(lines.len());
    for (l, line) in lines.iter().enumerate() {
        units(ledger, 1)?;
        let li = l as u32;
        let [a, b] = line.ends;
        if a as usize >= n || b as usize >= n || point_of[a as usize] == point_of[b as usize] {
            return fail(CoincidentError::Line(li));
        }
        let knots = line.pcurve.knots();
        let last = knots.len() - 1;
        let fits = |v: u32, k: usize| {
            line.pcurve
                .knot_point(k)
                .is_some_and(|p| same_bits(p, points[point_of[v as usize] as usize].uv))
        };
        if !fits(a, 0) || !fits(b, last) {
            return fail(CoincidentError::Line(li));
        }
        let mut cs = vec![Cut {
            s: knots[0],
            vert: a,
        }];
        let mut inner: Vec<Cut> = line_vert
            .range((li, 0)..=(li, u64::MAX))
            .map(|(&(_, key), &vert)| Cut {
                s: f64::from_bits(key),
                vert,
            })
            .collect();
        inner.sort_by(|x, y| x.s.total_cmp(&y.s));
        cs.extend(inner);
        cs.push(Cut {
            s: knots[last],
            vert: b,
        });
        first_bundle.push(next_id(bundles.len())?);
        for (part, w) in cs.windows(2).enumerate() {
            units(ledger, 1)?;
            let (p, bundle) = (next_id(pieces.len())?, next_id(bundles.len())?);
            pieces.push(Piece {
                of: PieceOf::Line {
                    line: li,
                    part: part as u32,
                },
                ends: [w[0].vert, w[1].vert],
                reuses: Some((li, [w[0].s, w[1].s])),
                bundle,
                forward: true,
            });
            bundles.push(Bundle {
                members: vec![p],
                ends: [point_of[w[0].vert as usize], point_of[w[1].vert as usize]],
                wind: line.wind,
            });
        }
        cuts.push(cs);
    }

    // The arcs.
    let mut zeros: Vec<Zero> = Vec::new();
    let mut free: Vec<u32> = Vec::new();
    for (i, arc) in arcs.iter().enumerate() {
        units(ledger, 1)?;
        let ai = i as u32;
        let bad = CoincidentError::Arc(ai);
        let [a, b] = arc.ends;
        if a as usize >= n || b as usize >= n || a == b {
            return fail(bad);
        }
        let (pa, pb) = (point_of[a as usize], point_of[b as usize]);
        if pa == pb {
            zeros.push(Zero {
                of: ZeroOf::Arc(ai),
                ends: [a, b],
                point: pa,
            });
            continue;
        }
        if arc.along.is_empty() {
            free.push(ai);
            continue;
        }
        if arc.along.iter().any(|r| r.line as usize >= lines.len()) {
            return fail(bad);
        }
        // The cut index of each end on the chain's first and last lines,
        // and of each join, as the chain runs.
        let at = |ledger: &mut Ledger<'_>, r: &Along, v: u32| -> Res<Option<usize>> {
            let cs = &cuts[r.line as usize];
            units(ledger, cs.len() as u64)?;
            Ok(cs
                .iter()
                .position(|c| point_of[c.vert as usize] == point_of[v as usize]))
        };
        let ends_of = |r: &Along| {
            let cs = &cuts[r.line as usize];
            if r.forward {
                (0, cs.len() - 1)
            } else {
                (cs.len() - 1, 0)
            }
        };
        let k = arc.along.len();
        let mut part = 0u32;
        for (j, r) in arc.along.iter().enumerate() {
            let cs = &cuts[r.line as usize];
            let (mut from, mut to) = ends_of(r);
            if j == 0 {
                let Some(x) = at(ledger, r, a)? else {
                    return fail(bad);
                };
                from = x;
            }
            if j + 1 == k {
                let Some(x) = at(ledger, r, b)? else {
                    return fail(bad);
                };
                to = x;
            } else {
                let next = &arc.along[j + 1];
                let (start, _) = ends_of(next);
                let join = cuts[next.line as usize][start].vert;
                if point_of[cs[to].vert as usize] != point_of[join as usize] {
                    return fail(bad);
                }
            }
            let ok = if r.forward { from < to } else { from > to };
            if !ok {
                return fail(bad);
            }
            let steps: Vec<usize> = if r.forward {
                (from..to).collect()
            } else {
                (to..from).rev().collect()
            };
            for (q, &m) in steps.iter().enumerate() {
                units(ledger, 1)?;
                let (x, y) = (cs[m], cs[m + 1]);
                let (mut s, mut e) = if r.forward { (x, y) } else { (y, x) };
                if j == 0 && q == 0 {
                    s.vert = a;
                }
                if j + 1 == k && q + 1 == steps.len() {
                    e.vert = b;
                }
                let bundle = first_bundle[r.line as usize] + m as u32;
                let p = next_id(pieces.len())?;
                pieces.push(Piece {
                    of: PieceOf::Arc { arc: ai, part },
                    ends: [s.vert, e.vert],
                    reuses: Some((r.line, [s.s, e.s])),
                    bundle,
                    forward: r.forward,
                });
                part += 1;
                let bd = &mut bundles[bundle as usize];
                bd.members.push(p);
                let step = if r.forward {
                    arc.wind
                } else {
                    arc.wind
                        .checked_neg()
                        .ok_or(Stop::Failed(CoincidentError::Wind(bundle)))?
                };
                bd.wind = bd
                    .wind
                    .checked_add(step)
                    .ok_or(Stop::Failed(CoincidentError::Wind(bundle)))?;
            }
        }
    }
    for &ai in &free {
        units(ledger, 1)?;
        let arc = &arcs[ai as usize];
        let (p, bundle) = (next_id(pieces.len())?, next_id(bundles.len())?);
        pieces.push(Piece {
            of: PieceOf::Arc { arc: ai, part: 0 },
            ends: arc.ends,
            reuses: None,
            bundle,
            forward: true,
        });
        bundles.push(Bundle {
            members: vec![p],
            ends: [
                point_of[arc.ends[0] as usize],
                point_of[arc.ends[1] as usize],
            ],
            wind: arc.wind,
        });
    }
    for (v, p) in verts.iter().enumerate() {
        if let Place::Pin { line, s } = *p {
            units(ledger, 1)?;
            let w = line_vert[&(line, bits(s))];
            zeros.push(Zero {
                of: ZeroOf::Pin(v as u32),
                ends: [w, v as u32],
                point: point_of[v],
            });
        }
    }

    // The points' vertices.
    let mut touched = vec![false; points.len()];
    for p in &pieces {
        for &v in &p.ends {
            let pt = point_of[v as usize] as usize;
            touched[pt] = true;
            points[pt].verts.push(v);
        }
    }
    for z in &zeros {
        for &v in &z.ends {
            points[z.point as usize].verts.push(v);
        }
    }
    for (pt, p) in points.iter_mut().enumerate() {
        p.verts.sort_unstable();
        p.verts.dedup();
        p.contracted = !touched[pt] && !p.verts.is_empty();
    }

    // The arrangement's input: the points, then the bends.
    let mut nodes: Vec<[f64; 2]> = points.iter().map(|p| p.uv).collect();
    let mut spans: Vec<Edge> = Vec::new();
    let mut span_bundle: Vec<u32> = Vec::new();
    for (bi, bd) in bundles.iter().enumerate() {
        let lead = pieces[bd.members[0] as usize];
        let mut chain = vec![bd.ends[0]];
        if let (PieceOf::Line { line, .. }, Some((_, [s0, s1]))) = (lead.of, lead.reuses) {
            let pc = lines[line as usize].pcurve;
            for (k, &t) in pc.knots().iter().enumerate() {
                if s0 < t && t < s1 {
                    units(ledger, 1)?;
                    let Some(uv) = pc.knot_point(k) else {
                        return fail(CoincidentError::Line(line));
                    };
                    chain.push(next_id(nodes.len())?);
                    nodes.push(uv);
                }
            }
        }
        chain.push(bd.ends[1]);
        for w in chain.windows(2) {
            units(ledger, 1)?;
            spans.push(Edge {
                ends: [w[0], w[1]],
                wind: bd.wind,
            });
            span_bundle.push(bi as u32);
        }
    }

    Ok(Built {
        point_of,
        pins,
        points,
        pieces,
        zeros,
        bundles,
        span_bundle,
        nodes,
        spans,
    })
}

#[cfg(test)]
mod tests;
