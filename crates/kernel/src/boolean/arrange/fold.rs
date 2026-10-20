//! Zero-area regions of a face's arrangement: found by record, dropped
//! or folded, the survivors of each fold chosen, and the fold checked.
//!
//! **Input** ([`FoldInput`]). A face realised by
//! [`coincident`](super::coincident): its
//! pieces, its bundles (the pieces on one line piece, one edge of the
//! planar arrangement) and the arrangement with its windings. What that
//! leaves open the caller gives, from the records that made the face:
//! - the **side order** of each bundle's members ([`FoldInput::side`]):
//!   a rank per piece, distinct within its bundle, lower to the right of
//!   the bundle's direction, as the tie records that created the curves
//!   say;
//! - the zero-length edges **contracted** before the regions are traced
//!   ([`FoldInput::contract`]): those bounding a zero-area region (two
//!   touching edges at one cut point, two corner records of one vertex),
//!   grouped by the vertex's fan walk; the others stay;
//! - where each line, arc and vertex comes from and its place in the
//!   geometric order ([`Source`]), for the survivors;
//! - per piece, the kept faces across its edge other than this face
//!   ([`FoldInput::across`]), for the check;
//! - the **keep rule** on windings ([`FoldInput::keep`]), the operation's.
//!
//! **Regions.** Between consecutive members of a bundle lies a sliver of
//! zero area. The face's cycles are traced on the pieces: at each point
//! the rays of its bundles in the arrangement's rotation order, each
//! bundle's members spread by side order (counter-clockwise: from the
//! right of a bundle leaving the point, from the left of one reaching
//! it); a vertex's rays are its own and those of the vertices it is
//! contracted with, and a cycle turns at a vertex to the next of them
//! clockwise, so distinct vertices at one point stay apart. A walk along
//! a piece with a member on its left borders a sliver; its **partner** is
//! that member walked the other way, the sliver's other side. A cycle all
//! of whose walks border slivers bounds zero area; cycles joined by
//! partners bound one region, and a region all of whose cycles do so is
//! a **zero-area region** ([`ZeroRegion`]): a disk (one cycle) or an
//! annulus (two), never built as a face. Cycles with a walk bordering
//! positive area bound the arrangement's regions (a sliver part there is
//! a slit). Nothing here compares positions: the members of a bundle
//! share their line piece's p-curve by record.
//!
//! **Labels.** A sliver's winding is the one right of its bundle plus the
//! steps of the members right of it; a region's slivers must agree. The
//! keep rule on it decides: **discarded**, the region is dropped (its
//! pieces stay distinct edges); **kept**, it is **folded**: each walk is
//! paired with its partner, the two pieces identified into one edge and
//! their ends at each end of the bundle into one vertex.
//!
//! **Survivors.** Of an identified edge: the operand edge's piece (`A`'s
//! before `B`'s), else the piece first in the geometric order; of a
//! vertex: the operand vertex (`A`'s before `B`'s), else the first in the
//! geometric order. A line's pin vertex comes after the face's vertices.
//! Equal places (distinct entities given one) fall to ids. The others
//! merge into them ([`Fold::piece_into`], [`Fold::vert_into`]).
//!
//! **Check** (on every kept region, failures [`FoldError::is_inconsistent`]):
//! the identified vertices are at one point, bit for bit; the quotient,
//! vertices up to the identifications and one edge per pair, is
//! connected and a tree or a cycle ([`Shape`]); and each identified edge
//! has exactly two kept faces or none: the faces across its pieces, and
//! this face's kept regions beside its bundle's outermost members.
//!
//! **Order.** Walks are numbered `2·piece`, along the piece, and
//! `2·piece + 1` against it; cycles in the order of their least walk;
//! regions of their least cycle; identified edges of their least piece.
//! The numbering follows the input's; the outcome reads ids only where
//! sources tie.
//!
//! **Work.** One pass on the operation's budget, one item: a unit per
//! bundle member sorted, contraction, ray, walk traced, walk grouped, walk
//! of a region labelled, pair and identified edge's member, and per vertex
//! and piece for the survivors.

use std::collections::BTreeMap;

use super::coincident::{Coincident, FaceInput, PieceOf};
use crate::infra::budget::{Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::{Internal, KernelError};
use crate::topo::arrange2d::NONE;

/// Which body an entity comes from, in survivor order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Operand {
    /// An entity of operand `A`.
    A,
    /// An entity of operand `B`.
    B,
    /// A new one: an arc, a record.
    New,
}

/// Where an entity comes from and its place in the geometric order
/// (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Source {
    /// Its body.
    pub operand: Operand,
    /// Its place in the geometric order.
    pub order: u32,
}

/// A realised face and what its zero-area regions need from the records
/// (module docs).
#[derive(Clone, Copy)]
pub struct FoldInput<'a> {
    /// The face as given to [`coincident::realise`](super::coincident::realise).
    pub face: &'a FaceInput<'a>,
    /// What that made of it.
    pub realised: &'a Coincident,
    /// Per piece, its rank in its bundle's side order, lower to the right.
    pub side: &'a [u32],
    /// The zero-length edges contracted, by index.
    pub contract: &'a [u32],
    /// Per line, its source.
    pub lines: &'a [Source],
    /// Per arc, its place in the geometric order.
    pub arcs: &'a [u32],
    /// Per vertex of the face, its source.
    pub verts: &'a [Source],
    /// Per piece, the kept faces across its edge other than this face.
    pub across: &'a [u32],
    /// Whether a region of this winding is kept.
    pub keep: &'a (dyn Fn(i64) -> bool + Sync),
}

/// A piece walked along its own direction or against it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Walk {
    /// The piece.
    pub piece: u32,
    /// Whether it runs from `ends[0]` to `ends[1]`.
    pub forward: bool,
}

/// What a kept region folds onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A tree (a disk's fold).
    Tree,
    /// A cycle (an annulus's).
    Cycle,
}

/// What became of a zero-area region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fate {
    /// Discarded: dropped.
    Dropped,
    /// Kept: folded onto this.
    Folded(Shape),
}

/// A zero-area region (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZeroRegion {
    /// Its boundary cycles, each from its least walk, the region on the
    /// left.
    pub cycles: Vec<Vec<Walk>>,
    /// Its winding number.
    pub winding: i64,
    /// Dropped or folded.
    pub fate: Fate,
}

/// Pieces identified into one edge by folds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collapsed {
    /// The survivor.
    pub into: u32,
    /// The pieces, ascending, the survivor among them.
    pub members: Vec<u32>,
    /// Its kept faces: 0 or 2.
    pub faces: u32,
}

/// A face's zero-area regions dropped or folded (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    /// The zero-area regions.
    pub regions: Vec<ZeroRegion>,
    /// The identified edges.
    pub edges: Vec<Collapsed>,
    /// Per piece, the piece it merges into (itself if it survives).
    pub piece_into: Vec<u32>,
    /// Per vertex (the face's, then the pins), the vertex it merges into,
    /// by a fold or a contraction (itself if it survives).
    pub vert_into: Vec<u32>,
}

/// Why the face's zero-area regions can't be settled (module docs).
#[derive(Clone, Debug, PartialEq)]
pub enum FoldError {
    /// A column's length doesn't match the face's, or the realisation
    /// isn't the face's.
    Input,
    /// Two members of this bundle have one side rank.
    Side(u32),
    /// This entry of the contracted list names no zero-length edge.
    Contract(u32),
    /// The slivers of this region disagree on its winding.
    Winding(u32),
    /// A winding number leaves `i64`.
    WindingRange,
    /// This kept region identifies two vertices at different points.
    Points(u32),
    /// This kept region's quotient is neither a tree nor a cycle.
    Quotient(u32),
    /// This identified edge (by its survivor) has this many kept faces.
    Faces {
        /// The survivor.
        edge: u32,
        /// Its kept faces.
        faces: u32,
    },
    /// The budget ran out, or a fixed limit was reached
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

impl FoldError {
    /// Whether the face's decisions contradict each other (a disagreeing
    /// winding, or a kept region failing the check): the ladder's
    /// [`Internal::Inconsistent`], not an input defect.
    pub fn is_inconsistent(&self) -> bool {
        matches!(
            self,
            FoldError::Winding(_)
                | FoldError::Points(_)
                | FoldError::Quotient(_)
                | FoldError::Faces { .. }
        )
    }

    /// The kernel's error for it: [`Internal::Inconsistent`] where
    /// [`Self::is_inconsistent`], the budget's where it ran out, else
    /// `None` (the caller's input).
    pub fn kernel_error(&self) -> Option<KernelError> {
        match self {
            FoldError::OutOfBudget(e) => Some(e.clone()),
            e if e.is_inconsistent() => Some(KernelError::Internal(Internal::Inconsistent)),
            _ => None,
        }
    }
}

/// The face's zero-area regions found, dropped or folded and checked
/// (module docs), charged to `work`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the boolean's face arrangements call it as they land"
    )
)]
pub(crate) fn fold(input: &FoldInput<'_>, work: &mut Work) -> Result<Fold, FoldError> {
    work.pass(
        Pass {
            stage: "zero-area regions",
            rung: 0,
        },
        &[()],
        |_, ledger| build(input, ledger),
    )
    .map_err(|halt| match halt {
        Halt::Failed { error, .. } => error,
        Halt::OutOfBudget(e) => FoldError::OutOfBudget(e),
    })
    .map(|mut one| one.swap_remove(0))
}

type Res<T> = Result<T, Stop<FoldError>>;

fn fail<T>(e: FoldError) -> Res<T> {
    Err(Stop::Failed(e))
}

fn units(ledger: &mut Ledger<'_>, n: usize) -> Res<()> {
    ledger.charge(Step::Work, n as u64).map_err(Stop::from)
}

/// Union-find over `0..n`, each class named by its least member.
struct Classes(Vec<u32>);

impl Classes {
    fn new(n: usize) -> Classes {
        Classes((0..n as u32).collect())
    }

    fn find(&mut self, x: u32) -> u32 {
        let mut r = x;
        while self.0[r as usize] != r {
            r = self.0[r as usize];
        }
        let mut y = x;
        while self.0[y as usize] != r {
            let next = self.0[y as usize];
            self.0[y as usize] = r;
            y = next;
        }
        r
    }

    fn union(&mut self, x: u32, y: u32) {
        let (a, b) = (self.find(x), self.find(y));
        if a < b {
            self.0[b as usize] = a;
        } else if b < a {
            self.0[a as usize] = b;
        }
    }
}

/// What a quotient of `n` vertices with these edges is: a connected tree
/// or cycle, else `None`.
fn shape(n: usize, edges: &[[u32; 2]]) -> Option<Shape> {
    let mut comps = Classes::new(n);
    let mut degree = vec![0usize; n];
    for &[a, b] in edges {
        comps.union(a, b);
        degree[a as usize] += 1;
        degree[b as usize] += 1;
    }
    let connected = n > 0 && (0..n as u32).all(|v| comps.find(v) == 0);
    if !connected {
        None
    } else if edges.len() + 1 == n {
        Some(Shape::Tree)
    } else if edges.len() == n && degree.iter().all(|&d| d == 2) {
        Some(Shape::Cycle)
    } else {
        None
    }
}

/// The face's bundles as the trace reads them.
struct Sides<'a> {
    realised: &'a Coincident,
    /// Per bundle, its members by side rank, right to left.
    sorted: Vec<Vec<u32>>,
    /// Per piece, its place in its bundle's `sorted`.
    place: Vec<usize>,
    /// Per bundle, the windings right of each member and left of the last
    /// (`sorted.len() + 1` entries): the gaps' windings, slivers inside.
    gap: Vec<Vec<i64>>,
}

impl Sides<'_> {
    fn piece(&self, w: u32) -> u32 {
        w / 2
    }

    /// The vertex walk `w` leaves.
    fn origin(&self, w: u32) -> u32 {
        self.realised.pieces[(w / 2) as usize].ends[(w % 2) as usize]
    }

    /// The vertex walk `w` reaches.
    fn dest(&self, w: u32) -> u32 {
        self.origin(w ^ 1)
    }

    /// Whether walk `w` runs its bundle's way.
    fn along(&self, w: u32) -> bool {
        (w % 2 == 0) == self.realised.pieces[(w / 2) as usize].forward
    }

    /// The walk along piece `p` its bundle's way, or against it.
    fn walk(&self, p: u32, along: bool) -> u32 {
        2 * p + u32::from(self.realised.pieces[p as usize].forward != along)
    }

    /// The member on walk `w`'s left in its bundle, and the sliver
    /// between them as its gap index.
    fn left(&self, w: u32) -> Option<(u32, usize)> {
        let p = self.piece(w) as usize;
        let s = &self.sorted[self.realised.pieces[p].bundle as usize];
        let i = self.place[p];
        if self.along(w) {
            s.get(i + 1).map(|&q| (q, i + 1))
        } else {
            i.checked_sub(1).map(|j| (s[j], i))
        }
    }

    /// Walk `w`'s partner across its sliver, if it borders one.
    fn partner(&self, w: u32) -> Option<u32> {
        self.left(w).map(|(q, _)| self.walk(q, !self.along(w)))
    }
}

/// The regions (module docs), on one ledger.
fn build(input: &FoldInput<'_>, ledger: &mut Ledger<'_>) -> Res<Fold> {
    let c = input.realised;
    let face = input.face;
    let np = c.pieces.len();
    let nf = face.verts.len();
    let nv = c.point_of.len();
    if nv != nf + c.pins.len()
        || input.side.len() != np
        || input.across.len() != np
        || input.lines.len() != face.lines.len()
        || input.arcs.len() != face.arcs.len()
        || input.verts.len() != nf
        || 2 * np >= NONE as usize
    {
        return fail(FoldError::Input);
    }
    let arr = &c.arrangement;

    // The side order and the gaps' windings.
    let nb = c.bundles.len();
    let mut first_span = vec![NONE; nb];
    for (e, &b) in c.span_bundle.iter().enumerate().rev() {
        first_span[b as usize] = e as u32;
    }
    let mut sorted = Vec::with_capacity(nb);
    let mut place = vec![0usize; np];
    let mut gap = Vec::with_capacity(nb);
    for (b, bd) in c.bundles.iter().enumerate() {
        units(ledger, bd.members.len())?;
        let mut s = bd.members.clone();
        s.sort_by_key(|&p| input.side[p as usize]);
        if s.windows(2)
            .any(|w| input.side[w[0] as usize] == input.side[w[1] as usize])
        {
            return fail(FoldError::Side(b as u32));
        }
        for (i, &p) in s.iter().enumerate() {
            place[p as usize] = i;
        }
        let e = first_span[b];
        if e == NONE {
            return fail(FoldError::Input);
        }
        let right = arr.cycle_region(arr.cycle_of(2 * e + 1));
        let mut g = vec![arr.regions()[right as usize].winding];
        for &p in &s {
            let piece = &c.pieces[p as usize];
            let wind = match piece.of {
                PieceOf::Line { line, .. } => face.lines[line as usize].wind,
                PieceOf::Arc { arc, .. } => face.arcs[arc as usize].wind,
            };
            let step = if piece.forward {
                i64::from(wind)
            } else {
                -i64::from(wind)
            };
            let last = g[g.len() - 1];
            g.push(
                last.checked_add(step)
                    .ok_or(Stop::Failed(FoldError::WindingRange))?,
            );
        }
        sorted.push(s);
        gap.push(g);
    }
    let sides = Sides {
        realised: c,
        sorted,
        place,
        gap,
    };

    // Vertices contracted together.
    let mut node = Classes::new(nv);
    for (k, &z) in input.contract.iter().enumerate() {
        units(ledger, 1)?;
        let Some(zero) = c.zeros.get(z as usize) else {
            return fail(FoldError::Contract(k as u32));
        };
        node.union(zero.ends[0], zero.ends[1]);
    }

    // The rays at each vertex, counter-clockwise.
    let nw = 2 * np;
    let mut rays: Vec<Vec<u32>> = vec![Vec::new(); nv];
    let mut ray_at = vec![0usize; nw];
    for pt in 0..c.points.len() as u32 {
        for &h in arr.rotation(pt) {
            let b = c.span_bundle[arr.edge(h) as usize] as usize;
            let leaving = arr.forward(h);
            let s = &sides.sorted[b];
            units(ledger, s.len())?;
            for k in 0..s.len() {
                let p = if leaving { s[k] } else { s[s.len() - 1 - k] };
                let w = sides.walk(p, leaving);
                let v = node.find(sides.origin(w)) as usize;
                ray_at[w as usize] = rays[v].len();
                rays[v].push(w);
            }
        }
    }

    // The cycles.
    let mut cycle_of = vec![NONE; nw];
    let mut cycles: Vec<Vec<u32>> = Vec::new();
    for start in 0..nw as u32 {
        if cycle_of[start as usize] != NONE {
            continue;
        }
        let id = cycles.len() as u32;
        let mut cyc = Vec::new();
        let mut w = start;
        loop {
            units(ledger, 1)?;
            cycle_of[w as usize] = id;
            cyc.push(w);
            let twin = w ^ 1;
            let around = &rays[node.find(sides.origin(twin)) as usize];
            let i = ray_at[twin as usize];
            w = around[(i + around.len() - 1) % around.len()];
            if w == start {
                break;
            }
            if cycle_of[w as usize] != NONE {
                // Each walk has one successor and one predecessor, so a
                // walk is reached again only at its cycle's start.
                return fail(FoldError::Input);
            }
        }
        cycles.push(cyc);
    }

    // Cycles joined by partners; the zero-area regions.
    let mut joined = Classes::new(cycles.len());
    let mut pure = vec![true; cycles.len()];
    units(ledger, nw)?;
    for w in 0..nw as u32 {
        match sides.partner(w) {
            Some(q) => joined.union(cycle_of[w as usize], cycle_of[q as usize]),
            None => pure[cycle_of[w as usize] as usize] = false,
        }
    }
    let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for k in 0..cycles.len() as u32 {
        groups.entry(joined.find(k)).or_default().push(k);
    }

    let mut regions = Vec::new();
    let mut verts = node;
    let mut pieces = Classes::new(np);
    for members in groups.values() {
        if !members.iter().all(|&k| pure[k as usize]) {
            continue;
        }
        let rid = regions.len() as u32;
        let walks: Vec<u32> = members
            .iter()
            .flat_map(|&k| cycles[k as usize].iter().copied())
            .collect();
        units(ledger, walks.len())?;
        let mut winding = None;
        for &w in &walks {
            let Some((_, g)) = sides.left(w) else {
                continue;
            };
            let b = c.pieces[sides.piece(w) as usize].bundle as usize;
            let x = sides.gap[b][g];
            if winding.is_some_and(|y| y != x) {
                return fail(FoldError::Winding(rid));
            }
            winding = Some(x);
        }
        let winding = winding.unwrap_or(0);
        let fate = if (input.keep)(winding) {
            // The pairs, once each, and the quotient.
            let mut local: BTreeMap<u32, u32> = BTreeMap::new();
            for &w in &walks {
                let v = verts.find(sides.origin(w));
                let n = local.len() as u32;
                local.entry(v).or_insert(n);
            }
            let mut ident = Classes::new(local.len());
            let mut pairs = Vec::new();
            for &w in &walks {
                let Some(q) = sides.partner(w) else {
                    continue;
                };
                if q < w {
                    continue;
                }
                units(ledger, 1)?;
                for (x, y) in [
                    (sides.origin(w), sides.dest(q)),
                    (sides.dest(w), sides.origin(q)),
                ] {
                    let (px, py) = (c.point_of[x as usize], c.point_of[y as usize]);
                    let (ux, uy) = (c.points[px as usize].uv, c.points[py as usize].uv);
                    if ux[0].to_bits() != uy[0].to_bits() || ux[1].to_bits() != uy[1].to_bits() {
                        return fail(FoldError::Points(rid));
                    }
                    let lx = local[&verts.find(x)];
                    let ly = local[&verts.find(y)];
                    ident.union(lx, ly);
                }
                pairs.push((w, q));
            }
            let mut class: BTreeMap<u32, u32> = BTreeMap::new();
            for l in 0..local.len() as u32 {
                let r = ident.find(l);
                let n = class.len() as u32;
                class.entry(r).or_insert(n);
            }
            let edges: Vec<[u32; 2]> = pairs
                .iter()
                .map(|&(w, _)| {
                    [sides.origin(w), sides.dest(w)]
                        .map(|v| class[&ident.find(local[&verts.find(v)])])
                })
                .collect();
            let Some(s) = shape(class.len(), &edges) else {
                return fail(FoldError::Quotient(rid));
            };
            for &(w, q) in &pairs {
                verts.union(sides.origin(w), sides.dest(q));
                verts.union(sides.dest(w), sides.origin(q));
                pieces.union(sides.piece(w), sides.piece(q));
            }
            Fate::Folded(s)
        } else {
            Fate::Dropped
        };
        regions.push(ZeroRegion {
            cycles: members
                .iter()
                .map(|&k| {
                    cycles[k as usize]
                        .iter()
                        .map(|&w| Walk {
                            piece: w / 2,
                            forward: w % 2 == 0,
                        })
                        .collect()
                })
                .collect(),
            winding,
            fate,
        });
    }

    // Survivors.
    units(ledger, nv)?;
    let vsource = |v: usize| -> Source {
        input.verts.get(v).copied().unwrap_or(Source {
            operand: Operand::New,
            order: u32::MAX,
        })
    };
    let mut vbest: BTreeMap<u32, (Source, u32)> = BTreeMap::new();
    for v in 0..nv as u32 {
        let key = (vsource(v as usize), v);
        let e = vbest.entry(verts.find(v)).or_insert(key);
        if key < *e {
            *e = key;
        }
    }
    let vert_into: Vec<u32> = (0..nv as u32).map(|v| vbest[&verts.find(v)].1).collect();

    units(ledger, np)?;
    let psource = |p: usize| -> Source {
        match c.pieces[p].of {
            PieceOf::Line { line, .. } => input.lines[line as usize],
            PieceOf::Arc { arc, .. } => Source {
                operand: Operand::New,
                order: input.arcs[arc as usize],
            },
        }
    };
    let mut classes: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for p in 0..np as u32 {
        classes.entry(pieces.find(p)).or_default().push(p);
    }
    let mut piece_into: Vec<u32> = (0..np as u32).collect();
    let mut edges = Vec::new();
    for members in classes.into_values() {
        if members.len() < 2 {
            continue;
        }
        units(ledger, members.len())?;
        let into = members
            .iter()
            .copied()
            .min_by_key(|&p| (psource(p as usize), p))
            .unwrap_or(members[0]);
        let mut faces = 0u32;
        for &p in &members {
            piece_into[p as usize] = into;
            faces = faces
                .checked_add(input.across[p as usize])
                .ok_or(Stop::Failed(FoldError::Faces {
                    edge: into,
                    faces: u32::MAX,
                }))?;
        }
        // This face's kept regions beside the bundle's outermost members.
        let b = c.pieces[members[0] as usize].bundle as usize;
        let (s, g) = (&sides.sorted[b], &sides.gap[b]);
        for (end, w) in [(s[0], g[0]), (s[s.len() - 1], g[g.len() - 1])] {
            if members.contains(&end) && (input.keep)(w) {
                faces = faces.saturating_add(1);
            }
        }
        if faces != 0 && faces != 2 {
            return fail(FoldError::Faces { edge: into, faces });
        }
        edges.push(Collapsed {
            into,
            members,
            faces,
        });
    }

    Ok(Fold {
        regions,
        edges,
        piece_into,
        vert_into,
    })
}

#[cfg(test)]
mod tests;
