//! The label audit of a planar face's regions: each region's winding
//! number, read from the records, checked against the other operand by a
//! query at a point certified far from that operand's boundary; or, where
//! no such point exists, the region found thin (near a parallel sheet) or
//! within the tie band of its transversal arcs.
//!
//! **Input** ([`AuditInput`]). One face of an operand, a plane: its tile
//! (weights 1, so its UV map is a polynomial patch), and its regions to
//! audit ([`AuditRegion`]), each as its loops of UV points with the
//! winding number its label came from ([`AuditRegion::of`] reads one off
//! a face arrangement). Regions inside snapped zones are T0 by
//! construction and not given. What the records know of the face the
//! caller gives as well:
//! - the leaf pairs covering it that are not Empty ([`Leaf`]): the pair's
//!   box on this face, its kind (Simple, Near or Contact, [`Cover`]) and
//!   its partner face on the other operand;
//! - the ribbons of the coedges near it ([`Ribbon`]): a straight piece of
//!   the coedge's curve and its seam bound `β_c`;
//! - the other operand's boundary as triangles, oriented outward: after
//!   the snap (`solid`, a closed surface, the one queried) and before it
//!   (`before`, read for distances only);
//! - the bands ([`Bands`]): `ε_aud`, and the tie residual `ρ_tie`.
//!
//! **Boxes.** A region is searched in the dyadic squares of the face's UV,
//! starting at the finest level whose side holds the region's UV extent
//! (at most two by two squares cover it), each depth splitting a square
//! into its four quarters. A square whose interior no loop segment meets
//! and whose centre lies outside the region (both exact, flat predicates
//! on the stored UVs) is dropped; any other is tested whole: its points
//! are a superset of the region's there, so whatever is certified of the
//! square holds of them.
//! The square's centre is evaluated on the tile, certified (`p`, within
//! `e` of the exact point), and the square lies within `R` of `p`: `R =
//! L_u·h + L_v·h + e`, `h` half its UV side, `L_u` and `L_v` bounds on the
//! tile's partials (twice its net's largest step in each direction).
//!
//! **Distances** to the other operand's boundary (both triangle sets) are
//! bounded at `p`, `dist⁻ ≤ dist(p) ≤ dist⁺`, each triangle by its plane
//! distance where `p`'s foot is certified inside it, by its edges'
//! segment distances where certified outside, by the plane distance below
//! and the edges above where neither is certified; a triangle whose area
//! isn't certified non-zero is bounded below by its longest edge less its
//! width. Every bound is a certified `f64` bound rounded outward, so the
//! square's points lie between `dist⁻(p) − R` and `dist⁺(p) + R`.
//!
//! **Near and far, with hysteresis.** A square is
//! - **parallel** when a Near or Contact leaf's box meets it (closed
//!   boxes), else **transversal**: every non-Empty leaf covering it is
//!   Simple, so only Simple arcs and coedge ties bound it there;
//! - **far** when `dist⁻(p) − R` exceeds its threshold: `ε_aud/2` if
//!   parallel; if transversal its own tie band, the greatest of `ρ_tie` and
//!   the `β_c` of every ribbon not certified farther than `β_c` from it;
//! - else **near** when `dist⁺(p) + R ≤ ε_aud`;
//! - else split.
//!
//! Far is tested first: a square that is both is queried. Near and far
//! squares are final. A point at distance `d` from the boundary is near
//! once `R ≤ ε_aud − d` and far once `R < d − threshold`, so every square
//! is one or the other once `R < (ε_aud − threshold)/2`: the search ends
//! at a depth set by the region's extent, `ε_aud` and the threshold
//! ([`D_AUDIT`]), where the threshold is below `ε_aud` (a tie band at
//! `ε_aud` or above never settles).
//!
//! **Queries.** Of a region's far squares, in key order (depth, then `u`,
//! then `v`), the first whose query is accepted decides: the other
//! operand's winding number at the square's centre `p`, unmoved (`d = 0`),
//! counted along the first of three fixed directions (the generic
//! translations [`T`]) for which no sign needs the perturbation: the
//! signed crossings of the ray from `p` with the `solid`'s triangles, each
//! decided by exact flat predicates (the ray's line against each edge, the
//! triangle's facing along the ray, `p`'s side of its plane). A ray
//! through an edge or vertex, or along a plane, needs the perturbation:
//! the next direction is tried, then the next square. The square is far
//! from both triangle sets, so no boundary separates it from the region's
//! points in it: their winding is the query's, before and after the snap.
//! A query unequal to the region's winding is the ladder's signal
//! ([`AuditError::Mismatch`]). A far square whose three directions all
//! need the perturbation is split, its quarters (far as well) queried at
//! the next depth.
//!
//! **No far square** ([`Verdict`]). When every square left is near, the
//! region is **thin** if one of them is parallel, named with its partners
//! (the faces of the Near and Contact leaves meeting its parallel squares,
//! whatever its transversal ones), else it lies within the tie band of
//! its Simple arcs and coedges, and its label stands by the tie bound.
//! Both are the caller's to record.
//!
//! **Limits.** A region with squares still undecided, or far squares all
//! of whose queries needed the perturbation, at depth [`D_AUDIT`] (or at
//! the UV grid's finest level, [`MAX_LEVEL`]) reaches the audit's cap:
//! [`KernelError::OutOfBudget`] with [`Cause::SizeLimit`], at
//! [`Cap::AuditDepth`]. The flat queries are exact, so none is ever
//! unsettled for want of precision.
//!
//! **Order and work.** The regions are searched together, one pass per
//! depth over every region's squares in key order (region, then the
//! square's key), then one pass over the regions with far squares at that
//! depth, each querying its squares in turn. A square's near/far
//! certificate is its own kind in the unit table ([`AuditUnits`]): each
//! loop segment tested against the square, then, unless it is dropped,
//! the square itself, each leaf tested against it, each triangle and
//! ribbon whose distance it bounds, in parts of a unit ([`UNIT_PARTS`])
//! rounded up once per square. Its exact predicates, and
//! a query's (a query costs its predicates alone), are charged as
//! records' are, at the units of the stages they ran. Each region's
//! verdict depends on its own geometry alone; of several mismatches, the
//! one reported is the first region's in the caller's order.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use glam::DVec3;
use varde_decide::num::bounded::{Bounded, down, up};
use varde_decide::num::exact::{self, MAX_INPUT, Pred, value_sign};
use varde_decide::num::preds::{Orient2d, Orient3Dir, Orient3d};
use varde_decide::num::schedule::{UNIT_PARTS, UNITS};
use varde_decide::perturb::T;

#[cfg(doc)]
use varde_decide::num::schedule::AuditUnits;

use crate::geom::lattice::MAX_LEVEL;
use crate::geom::tile::Tile;
use crate::infra::budget::{Cap, Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::{Inconsistency, Internal, KernelError};
use crate::topo::arrange2d::{Arrangement, NONE};

#[cfg(doc)]
use crate::infra::error::Cause;

/// The deepest a region's search goes, in splits from its first squares.
///
/// Set from the depth the hysteresis needs (module docs): a square is
/// decided once its 3D side is below `(ε_aud − t)/2`, `t` the greater of
/// `ε_aud/2` and `ρ_tie`, so a region whose first squares have side `S₀`
/// needs the splits that take `S₀` below that. The audit's fixtures
/// measure no more (at `ε_aud = 2⁻¹¹` mm: the wedges at 1°, 0.1° and
/// 0.01° 7, 10 and 12 splits, the fin and the ledge 12, of 14 allowed;
/// the thin plate 4 of 6, and the plate at `0.75·ε_aud`, the slowest case
/// of the hysteresis, 6 of 6; at `ε_aud = 2⁻¹⁸` mm a plate at
/// `0.74·ε_aud` 5 of 6), and the depth depends on `S₀/ε_aud` alone. The
/// largest first square is the whole tile, `S₀ = CELL`; at the least
/// `ε_aud`, `5·10⁻⁶` mm (`ε = 10⁻⁵`), with the widest `ρ_tie`
/// (`2.5·ε_tie = 0.625·ε_aud`) it needs 43. Then the UV grid's finest
/// level is never reached first: the absolute level needed is the same
/// 43 for every `S₀`.
pub const D_AUDIT: u32 = 43;

/// The number of fixed directions a query may try at one point.
pub const DIRECTIONS: usize = 3;

/// A triangle, its corners counter-clockwise about its outward normal.
pub type Tri = [[f64; 3]; 3];

/// A region to audit: its loops in the face's UV (outer and inner alike,
/// each a closed polygon of its points; the region is where an odd number
/// of them wind) and the winding number its label came from.
#[derive(Clone, Debug, PartialEq)]
pub struct AuditRegion {
    /// Its loops, each at least two points.
    pub loops: Vec<Vec<[f64; 2]>>,
    /// Its winding number in the other operand.
    pub winding: i64,
}

impl AuditRegion {
    /// Region `region` of a face arrangement on the UV points `verts`: its
    /// outer cycle and holes as loops of their half-edges' start points.
    /// `None` for the unbounded region or one not there.
    pub fn of(arr: &Arrangement, verts: &[[f64; 2]], region: u32) -> Option<AuditRegion> {
        let r = arr.regions().get(region as usize)?;
        if r.outer == NONE {
            return None;
        }
        let cycle = |c: u32| -> Vec<[f64; 2]> {
            arr.cycle(c)
                .map(|h| verts[arr.origin(h) as usize])
                .collect()
        };
        let mut loops = vec![cycle(r.outer)];
        loops.extend(r.holes.iter().map(|&c| cycle(c)));
        Some(AuditRegion {
            loops,
            winding: r.winding,
        })
    }
}

/// The kind of a leaf pair that is not Empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cover {
    /// Transversal: no tangency.
    Simple,
    /// Co-oriented, within the gap bound.
    Near,
    /// Opposed, within the gap bound.
    Contact,
}

/// A leaf pair covering the face, not Empty: its box on this face and on
/// which face of the other operand its other box lies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leaf {
    /// Its box in the face's UV, the low corner then the high one.
    pub uv: [[f64; 2]; 2],
    /// Its kind.
    pub cover: Cover,
    /// The other operand's face.
    pub partner: u32,
}

/// A coedge's ribbon near the face: a straight piece of its edge's curve
/// and its seam bound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ribbon {
    /// The piece's ends.
    pub ends: [[f64; 3]; 2],
    /// Its seam bound `β_c`.
    pub beta: f64,
}

/// The audit's bands, in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bands {
    /// `ε_aud`: near at most this far, far beyond half of it.
    pub eps_aud: f64,
    /// `ρ_tie`, the residual of a tie consequence: the least tie band of
    /// a transversal square.
    pub rho_tie: f64,
}

/// One face's audit (module docs).
#[derive(Clone, Copy)]
pub struct AuditInput<'a> {
    /// The face's plane tile, its weights 1.
    pub tile: &'a Tile,
    /// The regions to audit.
    pub regions: &'a [AuditRegion],
    /// The leaf pairs covering the face that are not Empty.
    pub leaves: &'a [Leaf],
    /// The ribbons of the coedges near the face.
    pub ribbons: &'a [Ribbon],
    /// The other operand's boundary after the snap, closed: queried.
    pub solid: &'a [Tri],
    /// The other operand's boundary before the snap: distances only.
    pub before: &'a [Tri],
    /// The bands.
    pub bands: Bands,
}

/// A partner of a thin region: a face of the other operand and the kind
/// of the leaf naming it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Partner {
    /// The face.
    pub face: u32,
    /// Near (co-oriented) or Contact (opposed).
    pub cover: Cover,
}

/// How a region's label was audited (module docs).
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// A query at a far square agreed with it.
    Queried {
        /// The square's centre, in the face's UV.
        uv: [f64; 2],
        /// The direction the query was accepted along, an index of [`T`].
        direction: u8,
    },
    /// No far square, some near square parallel: thin, its partners (the
    /// faces of the Near and Contact leaves meeting its parallel squares),
    /// ascending.
    Thin {
        /// Its partners.
        partners: Vec<Partner>,
    },
    /// No far square, every square transversal: within the tie band of
    /// its Simple arcs and coedges.
    TieBound,
}

/// A region's audit: its verdict and what the search took.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionAudit {
    /// The verdict.
    pub verdict: Verdict,
    /// The deepest split reached, from its first squares.
    pub depth: u32,
    /// The squares tested.
    pub boxes: u64,
}

/// What of the input the audit doesn't take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    /// More regions, leaves, ribbons or triangles than `u32` ids hold.
    TooMany,
    /// The tile isn't polynomial (a weight other than 1).
    Tile,
    /// A band isn't finite, `ε_aud` isn't positive or `ρ_tie` is negative.
    Bands,
    /// This region has no loop, a loop of fewer than two points, or a
    /// point outside `[0, 1]²`.
    Region(u32),
    /// This leaf's box isn't finite or is inverted.
    Leaf(u32),
    /// This ribbon has a coordinate past the exact predicates' range or a
    /// seam bound that isn't finite and non-negative.
    Ribbon(u32),
    /// This triangle (of `solid`, then `before`) has a coordinate that
    /// isn't finite or lies past the exact predicates' range.
    Triangle(u32),
}

/// Why the audit gives no verdicts.
#[derive(Clone, Debug, PartialEq)]
pub enum AuditError {
    /// The input isn't of the kind the audit takes.
    Input(InputError),
    /// The query at a far square of this region disagrees with its
    /// winding number: the ladder's signal.
    Mismatch {
        /// The region.
        region: u32,
        /// Its winding number.
        label: i64,
        /// The query's.
        query: i64,
    },
    /// The budget ran out, or the audit's depth was reached
    /// ([`KernelError::OutOfBudget`]).
    OutOfBudget(KernelError),
}

impl AuditError {
    /// The kernel's error for it: [`Internal::Inconsistent`] for a
    /// mismatch, of the proxy class until the ladder classifies the
    /// decisions it read; the budget's where it ran out; else `None` (the
    /// caller's input).
    pub fn kernel_error(&self) -> Option<KernelError> {
        match self {
            AuditError::Input(_) => None,
            AuditError::Mismatch { .. } => Some(KernelError::Internal(Internal::Inconsistent(
                Inconsistency::Proxy,
            ))),
            AuditError::OutOfBudget(e) => Some(e.clone()),
        }
    }
}

/// The audit of the regions of one face (module docs), charged to `work`:
/// a verdict per region, in their order; or the first input it doesn't
/// take, else the first mismatch or limit in pass order.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the boolean's classification calls it as it lands"
    )
)]
pub(crate) fn audit(
    input: &AuditInput<'_>,
    work: &mut Work,
) -> Result<Vec<RegionAudit>, AuditError> {
    validate(input)?;
    let face = Face::new(input).ok_or(AuditError::Input(InputError::Tile))?;
    let mut search: Vec<Search> = input
        .regions
        .iter()
        .map(|r| Search::start(&r.loops))
        .collect();
    let mut depth = 0u32;
    loop {
        // One pass over every square of this depth, in key order.
        let items: Vec<(u32, Cell)> = search
            .iter()
            .enumerate()
            .filter(|(_, s)| s.done.is_none())
            .flat_map(|(r, s)| s.cells.iter().map(move |&c| (r as u32, c)))
            .collect();
        if items.is_empty() {
            break;
        }
        let classes = run(work, "label audit", &items, |&(r, cell), m| {
            classify(&face, &input.regions[r as usize], cell, m)
        })?;
        let mut far: Vec<Vec<(Cell, [f64; 3])>> = vec![Vec::new(); search.len()];
        let mut split: Vec<Vec<Cell>> = vec![Vec::new(); search.len()];
        for (&(r, cell), class) in items.iter().zip(classes) {
            let (s, r) = (&mut search[r as usize], r as usize);
            s.boxes += 1;
            s.depth = depth;
            match class {
                Class::Drop => {}
                Class::Split => split[r].push(cell),
                Class::Far(p) => far[r].push((cell, p)),
                Class::Near(None) => {}
                Class::Near(Some(partners)) => {
                    s.parallel = true;
                    s.partners.extend(partners);
                }
            }
        }
        // Then one pass over the regions with far squares, or with squares
        // left where none can follow.
        // No square of the region may split past the cap or the UV grid.
        let at_cap = |s: &Search| depth >= D_AUDIT || s.level + depth >= MAX_LEVEL;
        let asked: Vec<u32> = (0..search.len() as u32)
            .filter(|&r| {
                let (s, r) = (&search[r as usize], r as usize);
                s.done.is_none() && (!far[r].is_empty() || (at_cap(s) && !split[r].is_empty()))
            })
            .collect();
        let answers = run(work, "label audit queries", &asked, |&r, m| {
            let capped = at_cap(&search[r as usize]);
            let label = input.regions[r as usize].winding;
            for &(cell, p) in &far[r as usize] {
                for (k, d) in T.iter().enumerate() {
                    if let Some(w) = winding(input.solid, p, *d, m)? {
                        if w != label {
                            return Err(Stop::Failed(AuditError::Mismatch {
                                region: r,
                                label,
                                query: w,
                            }));
                        }
                        return Ok(Some(Verdict::Queried {
                            uv: cell.centre(),
                            direction: k as u8,
                        }));
                    }
                }
            }
            if capped {
                return Err(Stop::Cap(Cap::AuditDepth));
            }
            Ok(None)
        })?;
        for (&r, answer) in asked.iter().zip(answers) {
            search[r as usize].done = answer;
        }
        for (r, s) in search.iter_mut().enumerate() {
            if s.done.is_some() {
                continue;
            }
            // Rejected far squares split as undecided ones do.
            let mut next: Vec<Cell> = split[r]
                .iter()
                .chain(far[r].iter().map(|(c, _)| c))
                .flat_map(|c| c.quarters())
                .collect();
            next.sort_unstable();
            if next.is_empty() {
                s.done = Some(if s.parallel {
                    Verdict::Thin {
                        partners: std::mem::take(&mut s.partners).into_iter().collect(),
                    }
                } else {
                    Verdict::TieBound
                });
            }
            s.cells = next;
        }
        depth += 1;
    }
    Ok(search
        .into_iter()
        .map(|s| RegionAudit {
            // Every region ends with a verdict: an empty search is one
            // with no square left, which the loop above gives one.
            verdict: s.done.unwrap_or(Verdict::TieBound),
            depth: s.depth,
            boxes: s.boxes,
        })
        .collect())
}

fn validate(input: &AuditInput<'_>) -> Result<(), AuditError> {
    let bad = |e| Err(AuditError::Input(e));
    let counts = [
        input.regions.len(),
        input.leaves.len(),
        input.ribbons.len(),
        input.solid.len().saturating_add(input.before.len()),
    ];
    if counts.iter().any(|&n| n > NONE as usize) {
        return bad(InputError::TooMany);
    }
    let b = input.bands;
    if !(b.eps_aud.is_finite() && b.eps_aud > 0.0 && b.rho_tie.is_finite() && b.rho_tie >= 0.0) {
        return bad(InputError::Bands);
    }
    for (i, r) in input.regions.iter().enumerate() {
        let ok = !r.loops.is_empty()
            && r.loops.iter().all(|l| {
                l.len() >= 2
                    && l.iter()
                        .all(|p| p.iter().all(|&x| (0.0..=1.0).contains(&x)))
            });
        if !ok {
            return bad(InputError::Region(i as u32));
        }
    }
    for (i, l) in input.leaves.iter().enumerate() {
        let [lo, hi] = l.uv;
        let ok = (0..2).all(|k| lo[k].is_finite() && hi[k].is_finite() && lo[k] <= hi[k]);
        if !ok {
            return bad(InputError::Leaf(i as u32));
        }
    }
    let in_range = |p: &[f64; 3]| p.iter().all(|x| x.is_finite() && x.abs() <= MAX_INPUT);
    for (i, r) in input.ribbons.iter().enumerate() {
        if !(r.ends.iter().all(in_range) && r.beta.is_finite() && r.beta >= 0.0) {
            return bad(InputError::Ribbon(i as u32));
        }
    }
    for (i, t) in input.solid.iter().chain(input.before).enumerate() {
        if !t.iter().all(in_range) {
            return bad(InputError::Triangle(i as u32));
        }
    }
    Ok(())
}

/// The face as the squares read it: the tile and the bounds on its
/// partials, with the rest of the input.
struct Face<'a> {
    input: &'a AuditInput<'a>,
    /// Bounds on `|x_u|` and `|x_v|` over the tile.
    lip: [f64; 2],
}

impl<'a> Face<'a> {
    /// `None` unless the tile's weights are all 1.
    fn new(input: &'a AuditInput<'a>) -> Option<Face<'a>> {
        let net = input.tile.net();
        if net.iter().any(|p| p[3] != 1.0) {
            return None;
        }
        let pt = |i: usize, j: usize| bv(input.tile.point(i, j));
        // Twice the net's largest step in each direction.
        let step = |a: V, b: V| up(2.0 * upper(norm(&vsub(&b, &a))));
        let mut lip = [0.0f64; 2];
        for i in 0..3 {
            for j in 0..3 {
                if i < 2 {
                    lip[0] = most(lip[0], step(pt(i, j), pt(i + 1, j)));
                }
                if j < 2 {
                    lip[1] = most(lip[1], step(pt(i, j), pt(i, j + 1)));
                }
            }
        }
        Some(Face { input, lip })
    }
}

/// One region's search state.
struct Search {
    /// The level of its first squares.
    level: u32,
    /// The squares of the current depth, in key order.
    cells: Vec<Cell>,
    /// Its verdict, once it has one.
    done: Option<Verdict>,
    /// Whether a near square was parallel.
    parallel: bool,
    /// The partners named by its parallel near squares.
    partners: BTreeSet<Partner>,
    /// The deepest split reached and the squares tested.
    depth: u32,
    boxes: u64,
}

impl Search {
    /// The first squares: the finest level whose side holds the region's
    /// UV extent, the (at most two by two) squares of it meeting its box.
    fn start(loops: &[Vec<[f64; 2]>]) -> Search {
        let pts = || loops.iter().flatten();
        let lo = |k: usize| pts().map(|p| p[k]).fold(f64::INFINITY, least);
        let hi = |k: usize| pts().map(|p| p[k]).fold(f64::NEG_INFINITY, most);
        let (lo, hi) = ([lo(0), lo(1)], [hi(0), hi(1)]);
        // The extent exactly: the coordinates are validated finite.
        let extent = (0..2)
            .map(|k| exact::rat(hi[k]) - exact::rat(lo[k]))
            .max()
            .expect("two directions");
        let mut level = MAX_LEVEL;
        while level > 0 && exact::rat(side(level)) < extent {
            level -= 1;
        }
        let n = 1u64 << level;
        let index = |x: f64| ((x * n as f64).floor() as u64).min(n - 1);
        let mut cells = Vec::new();
        for i in index(lo[0])..=index(hi[0]) {
            for j in index(lo[1])..=index(hi[1]) {
                cells.push(Cell { level, i, j });
            }
        }
        Search {
            level,
            cells,
            done: None,
            parallel: false,
            partners: BTreeSet::new(),
            depth: 0,
            boxes: 0,
        }
    }
}

/// A dyadic square of the face's UV: `[i, i + 1]·2⁻ˡ × [j, j + 1]·2⁻ˡ`.
/// Ordered by level, then `u`, then `v`: its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Cell {
    level: u32,
    i: u64,
    j: u64,
}

impl Cell {
    /// Its low and high corners, exactly.
    fn bounds(self) -> [[f64; 2]; 2] {
        let s = side(self.level);
        [
            [self.i as f64 * s, self.j as f64 * s],
            [(self.i + 1) as f64 * s, (self.j + 1) as f64 * s],
        ]
    }

    /// Its centre, exactly (the level is at most [`MAX_LEVEL`]).
    fn centre(self) -> [f64; 2] {
        let h = side(self.level + 1);
        [(2 * self.i + 1) as f64 * h, (2 * self.j + 1) as f64 * h]
    }

    /// Its four quarters, in key order.
    fn quarters(self) -> [Cell; 4] {
        let (level, i, j) = (self.level + 1, 2 * self.i, 2 * self.j);
        [
            Cell { level, i, j },
            Cell { level, i, j: j + 1 },
            Cell { level, i: i + 1, j },
            Cell {
                level,
                i: i + 1,
                j: j + 1,
            },
        ]
    }
}

/// `2⁻ˡᵉᵛᵉˡ`, exactly.
fn side(level: u32) -> f64 {
    f64::from_bits(u64::from(1023 - level) << 52)
}

/// What a square is (module docs).
enum Class {
    /// Outside the region.
    Drop,
    /// Far, its centre's point.
    Far([f64; 3]),
    /// Near: parallel with the partners its leaves name, or transversal.
    Near(Option<Vec<Partner>>),
    /// Neither yet.
    Split,
}

type Res<T> = Result<T, Stop<AuditError>>;

/// A ledger with the parts of a unit charged so far.
struct Meter<'l, 'p> {
    ledger: &'l mut Ledger<'p>,
    parts: u64,
}

impl Meter<'_, '_> {
    /// Charges `p` parts of a unit before the work they stand for: the
    /// whole units they complete now, the rest with the next parts or at
    /// the item's end ([`Meter::finish`]).
    fn parts(&mut self, p: u64) -> Res<()> {
        self.parts = self.parts.saturating_add(p);
        let whole = self.parts / UNIT_PARTS;
        self.parts %= UNIT_PARTS;
        if whole > 0 {
            self.ledger.charge(Step::Work, whole)?;
        }
        Ok(())
    }

    /// The item's parts left over, rounded up to a unit: an item is
    /// charged its parts rounded up once.
    fn finish(&mut self) -> Res<()> {
        if self.parts > 0 {
            self.parts = 0;
            self.ledger.charge(Step::Work, 1)?;
        }
        Ok(())
    }

    /// The exact sign of `pred`, its filter charged before and any
    /// further stage after; `None` for an input that isn't finite (never
    /// on validated input).
    fn sign(&mut self, pred: &impl Pred) -> Res<Option<Ordering>> {
        let filter = UNITS.record.of(pred.kind()).filter;
        self.parts(filter)?;
        let (s, w) = value_sign(pred);
        self.parts(w.parts(&UNITS.record).saturating_sub(filter))?;
        Ok(s.ok())
    }
}

/// Runs a pass of `f` over `items` at rung 0.
fn run<T, R, F>(
    work: &mut Work,
    stage: &'static str,
    items: &[T],
    f: F,
) -> Result<Vec<R>, AuditError>
where
    T: Sync,
    R: Send,
    F: Fn(&T, &mut Meter<'_, '_>) -> Res<R> + Sync + Send,
{
    work.pass(Pass { stage, rung: 0 }, items, |item, ledger| {
        let mut m = Meter { ledger, parts: 0 };
        let out = f(item, &mut m)?;
        m.finish()?;
        Ok(out)
    })
    .map_err(|halt| match halt {
        Halt::Failed { error, .. } => error,
        Halt::OutOfBudget(e) => AuditError::OutOfBudget(e),
    })
}

/// The square `cell` of `region` (module docs).
fn classify(
    face: &Face<'_>,
    region: &AuditRegion,
    cell: Cell,
    m: &mut Meter<'_, '_>,
) -> Res<Class> {
    let [lo, hi] = cell.bounds();
    if !meets_region(region, lo, hi, m)? && !inside(region, cell.centre(), m)? {
        return Ok(Class::Drop);
    }
    // Then the square's near/far certificate, at the audit's own row.
    let units = &UNITS.audit;
    m.parts(units.square)?;
    let input = face.input;
    let [u, v] = cell.centre();
    let at = input.tile.eval(u, v);
    let p = at.map(|c| c.v);
    let e = at.iter().fold(0.0, |s, c| {
        if c.is_certified() {
            up(s + c.err())
        } else {
            f64::INFINITY
        }
    });
    let h = side(cell.level + 1);
    let reach = up(up(face.lip[0] * h) + up(face.lip[1] * h) + e);
    let (mut near_lo, mut near_hi) = (f64::INFINITY, f64::INFINITY);
    for t in input.solid.iter().chain(input.before) {
        m.parts(units.triangle)?;
        let [l, h] = tri_dist(p, t);
        near_lo = least(near_lo, l);
        near_hi = least(near_hi, h);
    }
    // Parallel where a Near or Contact leaf meets the square.
    let mut partners = Vec::new();
    for l in input.leaves {
        m.parts(units.test)?;
        let meets = (0..2).all(|k| l.uv[0][k] <= hi[k] && lo[k] <= l.uv[1][k]);
        if meets && l.cover != Cover::Simple {
            partners.push(Partner {
                face: l.partner,
                cover: l.cover,
            });
        }
    }
    let parallel = !partners.is_empty();
    let threshold = if parallel {
        input.bands.eps_aud / 2.0
    } else {
        let mut band = input.bands.rho_tie;
        for r in input.ribbons {
            m.parts(units.ribbon)?;
            let [l, _] = seg_dist(bv(p), r.ends[0], r.ends[1]);
            let clear = down(l - reach) > r.beta;
            if !clear && r.beta > band {
                band = r.beta;
            }
        }
        band
    };
    if down(near_lo - reach) > threshold {
        return Ok(Class::Far(p));
    }
    if up(near_hi + reach) <= input.bands.eps_aud {
        return Ok(Class::Near(parallel.then_some(partners)));
    }
    Ok(Class::Split)
}

/// Whether a segment of the region's loops meets the open square
/// `(lo, hi)`: their extents overlap openly in `u` and in `v`, and the
/// segment's line has corners strictly on both sides, exactly. A square
/// the loops only touch lies wholly in or out of the region.
fn meets_region(
    region: &AuditRegion,
    lo: [f64; 2],
    hi: [f64; 2],
    m: &mut Meter<'_, '_>,
) -> Res<bool> {
    for l in &region.loops {
        for (k, &a) in l.iter().enumerate() {
            let b = l[(k + 1) % l.len()];
            m.parts(UNITS.audit.test)?;
            let apart = (0..2).any(|c| least(a[c], b[c]) >= hi[c] || most(a[c], b[c]) <= lo[c]);
            if apart {
                continue;
            }
            let corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
            let mut sides = [false; 2];
            for q in corners {
                match m.sign(&Orient2d([a, b, q]))? {
                    Some(Ordering::Greater) => sides[0] = true,
                    Some(Ordering::Less) => sides[1] = true,
                    _ => {}
                }
            }
            if sides[0] && sides[1] {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Whether `p`, on no segment of the region's loops, lies in the region:
/// the parity of its crossings with the ray from `p` along `+u` over all
/// loops, each decided exactly (a segment counts where one end lies
/// strictly above `p`'s line and the other not).
fn inside(region: &AuditRegion, p: [f64; 2], m: &mut Meter<'_, '_>) -> Res<bool> {
    let mut odd = false;
    for l in &region.loops {
        for (k, &a) in l.iter().enumerate() {
            let b = l[(k + 1) % l.len()];
            m.parts(UNITS.audit.test)?;
            if (a[1] > p[1]) == (b[1] > p[1]) {
                continue;
            }
            let o = m.sign(&Orient2d([a, b, p]))?;
            let rising = b[1] > a[1];
            if (rising && o == Some(Ordering::Greater)) || (!rising && o == Some(Ordering::Less)) {
                odd = !odd;
            }
        }
    }
    Ok(odd)
}

/// The other operand's winding number at `p`, counted along `d` (module
/// docs): `None` where a sign needs the perturbation.
fn winding(solid: &[Tri], p: [f64; 3], d: DVec3, m: &mut Meter<'_, '_>) -> Res<Option<i64>> {
    let q = DVec3::from_array(p);
    let mut w = 0i64;
    for t in solid {
        let [a, b, c] = t.map(DVec3::from_array);
        // The side of the ray's line each edge passes.
        let across = |a, b| Orient3Dir { p: q, a, b, d };
        let s = [
            m.sign(&across(a, b))?,
            m.sign(&across(b, c))?,
            m.sign(&across(c, a))?,
        ];
        let [Some(s0), Some(s1), Some(s2)] = s else {
            return Ok(None);
        };
        let has = |o: Ordering| s0 == o || s1 == o || s2 == o;
        if has(Ordering::Greater) && has(Ordering::Less) {
            // The ray's line passes beside the triangle.
            continue;
        }
        if s0 == Ordering::Equal || s0 != s1 || s1 != s2 {
            // Through an edge or a corner.
            return Ok(None);
        }
        // `n·d` and `n·(q − a)`, `n = (b − a) × (c − a)` the triangle's
        // outward normal.
        let facing = m.sign(&Orient3Dir {
            p: a,
            a: b,
            b: c,
            d,
        })?;
        let side = m.sign(&Orient3d([a, b, c, q]))?;
        let (Some(facing), Some(side)) = (facing, side) else {
            return Ok(None);
        };
        if facing == Ordering::Equal || side == Ordering::Equal {
            return Ok(None);
        }
        // Ahead of `p` where `p` lies on the side of the plane the ray
        // leaves: an exit counts +1, an entry −1.
        if side != facing {
            w += if facing == Ordering::Greater { 1 } else { -1 };
        }
    }
    Ok(Some(w))
}

type V = [Bounded<f64>; 3];

fn bv(p: [f64; 3]) -> V {
    p.map(Bounded::exact)
}

fn vsub(a: &V, b: &V) -> V {
    [0, 1, 2].map(|i| a[i] - b[i])
}

fn vdot(a: &V, b: &V) -> Bounded<f64> {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn vcross(a: &V, b: &V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: &V) -> Bounded<f64> {
    vdot(a, a).sqrt_nonneg()
}

/// A certified lower bound of `b`'s true value, `−∞` if uncertified.
fn lower(b: Bounded<f64>) -> f64 {
    if b.is_certified() {
        down(b.v - b.err())
    } else {
        f64::NEG_INFINITY
    }
}

/// A certified upper bound of `b`'s true value, `+∞` if uncertified.
fn upper(b: Bounded<f64>) -> f64 {
    if b.is_certified() {
        up(b.v + b.err())
    } else {
        f64::INFINITY
    }
}

/// Bounds of a distance: `[lower, upper]`, the lower at least 0.
fn bounds(b: Bounded<f64>) -> [f64; 2] {
    [most(lower(b), 0.0), upper(b)]
}

/// Bounds of the distance from `p` to the segment `a b`.
fn seg_dist(p: V, a: [f64; 3], b: [f64; 3]) -> [f64; 2] {
    let (av, bvv) = (bv(a), bv(b));
    let w = vsub(&p, &av);
    let to_a = bounds(norm(&w));
    if a == b {
        return to_a;
    }
    let to_b = bounds(norm(&vsub(&p, &bvv)));
    let e = vsub(&bvv, &av);
    let ee = vdot(&e, &e);
    let t = vdot(&w, &e);
    // Above: any point of the segment, the one at the rounded clamped
    // parameter included (evaluated with its error).
    let tf = if ee.v > 0.0 {
        (t.v / ee.v).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let q: V = [0, 1, 2].map(|i| av[i] + Bounded::exact(tf) * e[i]);
    let hi = least(least(to_a[1], to_b[1]), upper(norm(&vsub(&p, &q))));
    // Below: the line's distance, the segment's where the foot lies on
    // it; an end's where the foot is certified beyond it.
    let lo = if upper(t) <= 0.0 {
        to_a[0]
    } else if lower(t - ee) >= 0.0 {
        to_b[0]
    } else {
        most(lower(norm(&vcross(&w, &e)) / norm(&e)), 0.0)
    };
    [lo, hi]
}

/// Bounds of the distance from `p` to the triangle `t` (module docs).
fn tri_dist(p: [f64; 3], t: &Tri) -> [f64; 2] {
    let pv = bv(p);
    let edges = [(0, 1), (1, 2), (2, 0)];
    let segs = edges.map(|(i, j)| seg_dist(pv, t[i], t[j]));
    let seg_lo = segs.iter().fold(f64::INFINITY, |s, d| least(s, d[0]));
    let seg_hi = segs.iter().fold(f64::INFINITY, |s, d| least(s, d[1]));
    let v = t.map(bv);
    let n = vcross(&vsub(&v[1], &v[0]), &vsub(&v[2], &v[0]));
    let nlen = norm(&n);
    if lower(nlen) > 0.0 {
        let h = bounds((vdot(&n, &vsub(&pv, &v[0])) / nlen).abs());
        let sides = edges.map(|(i, j)| {
            let s = vdot(&n, &vcross(&vsub(&v[j], &v[i]), &vsub(&pv, &v[i])));
            (lower(s) > 0.0, upper(s) < 0.0)
        });
        if sides.iter().all(|s| s.0) {
            [h[0], least(h[1], seg_hi)]
        } else if sides.iter().any(|s| s.1) {
            [seg_lo, seg_hi]
        } else {
            [h[0], seg_hi]
        }
    } else {
        // Every point of the triangle lies within its width of its
        // longest edge (the angles at that edge's ends are not obtuse).
        let lens = edges.map(|(i, j)| lower(norm(&vsub(&v[j], &v[i]))));
        let k = (0..3).fold(0, |k, i| if lens[i] > lens[k] { i } else { k });
        let width = if lens[k] > 0.0 {
            up(upper(nlen) / lens[k])
        } else {
            f64::INFINITY
        };
        [most(down(segs[k][0] - width), 0.0), seg_hi]
    }
}

/// The lesser of two values, by compare and select.
fn least(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// The greater of two values, by compare and select.
fn most(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// Hooks for the unit table's calibration benchmark
/// (`crates/kernel/benches/calibrate.rs`), which times squares of the
/// audit and queries at their centres against the units charged for
/// them. Not part of the kernel's interface.
#[doc(hidden)]
pub mod calibration {
    use super::*;
    use crate::geom::form::{FormKey, PlaneKey};
    use crate::geom::lattice::{CELL, Lattice};
    use crate::infra::budget::Budget;

    /// The plane `z = 0` on its base tile, and the UV of its point `(x,
    /// y, 0)` (exact for dyadic coordinates).
    pub fn plane() -> (Tile, impl Fn([f64; 2]) -> [f64; 2]) {
        let key = PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).expect("a plane");
        let lattice = Lattice::new(FormKey::Plane(key)).expect("its lattice");
        let tile = lattice.base_tile(0, 0).expect("its tile");
        let (o, [u, v, _]) = (lattice.origin(), lattice.axes());
        let uv = move |[x, y]: [f64; 2]| {
            let d = [x - o[0], y - o[1], -o[2]];
            let along = |a: [f64; 3]| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5;
            [along(u), along(v)]
        };
        (tile, uv)
    }

    /// The square of the face's UV at `level` and indices `(i, j)` that
    /// holds the UV point `uv`.
    pub fn cell_at(uv: [f64; 2], level: u32) -> (u32, u64, u64) {
        let n = (1u64 << level) as f64;
        (
            level,
            (uv[0] * n).floor() as u64,
            (uv[1] * n).floor() as u64,
        )
    }

    /// Classifies the squares `cells` (level, `u` index, `v` index) of
    /// region `region` of `input`, as the audit does, in one pass on a
    /// fresh budget: the units charged, or `None` if the input isn't
    /// taken or the pass doesn't finish.
    pub fn squares(
        input: &AuditInput<'_>,
        region: usize,
        cells: &[(u32, u64, u64)],
    ) -> Option<u64> {
        validate(input).ok()?;
        let face = Face::new(input)?;
        let r = input.regions.get(region)?;
        let cells: Vec<Cell> = cells
            .iter()
            .map(|&(level, i, j)| Cell { level, i, j })
            .collect();
        let mut work = Work::new(&Budget::DEFAULT);
        run(&mut work, "label audit", &cells, |&c, m| {
            classify(&face, r, c, m)
        })
        .ok()?;
        Some(work.used())
    }

    /// The other operand's winding number at `p` along direction `k` of
    /// [`T`] by `solid`, as a far square's query, in a pass of its own on
    /// a fresh budget: the winding (`None` where a sign needs the
    /// perturbation) and the units charged.
    pub fn query(solid: &[Tri], p: [f64; 3], k: usize) -> (Option<i64>, u64) {
        let mut work = Work::new(&Budget::DEFAULT);
        let out = run(&mut work, "label audit queries", &[()], |_, m| {
            winding(solid, p, T[k % DIRECTIONS], m)
        });
        (out.ok().and_then(|w| w[0]), work.used())
    }
}

#[cfg(test)]
mod tests;
