//! The body's check: the invariants every valid body keeps, run in order,
//! the first failure reported the same way on every run. It is the
//! kernel's trust boundary: an operand passes it before an operation
//! reads it, a constructed body before it is returned, and an operation's
//! result before it is returned.
//!
//! **The invariants** ([`Inv`]), in their order:
//! 1. topology: pairs, loops, fans, links and counts;
//! 2. bit-exact joins: curve and p-curve ends, tile rows, breakpoints;
//! 3. domain: loops inside their surface's lattice rectangle;
//! 4. trims: loops simple, disjoint and nested;
//! 5. seams: seam bounds, edge and vertex tolerances;
//! 6. regularity and continuity: folds, `G¹` across tile lines, same-form
//!    edges, certified continuity;
//! 7. forms and fits: surfaces on their forms, fitted deviations;
//! 8. orientation and regions (solids only): winding, shells, regions;
//! 9. contacts and embedding: contact records, adjacent and non-adjacent
//!    face pairs;
//! 10. names: face keys, tags, aliases, provenance.
//!
//! Each invariant is a hook ([`Invariant`]) checking items of the body
//! ([`Item`]: the body, its vertices, edges, coedges, loops, faces,
//! shells, regions or contact records), one kind of item after another
//! in the hook's order, each kind in one charged pass.
//!
//! **The first error.** The check stops at the first failure: of the
//! invariants, the first failing; within it, the first kind of item in
//! the hook's order with a failing item; of those items, the first in the
//! **geometric order** (`varde_decide::order`), so that the item reported
//! doesn't depend on the body's ids; within the item, the first of the
//! hook's own checks. The order of a kind's items:
//! - a vertex: its position, then its faces' keys;
//! - an edge: its lesser end, its other end, its curve's point at the
//!   middle of its parameter range, its two faces' keys;
//! - a coedge: its edge, its face's key, whether it runs from the edge's
//!   lesser end (not the stored direction, which the vertex ids choose);
//! - a loop: its face's key, outer loops first, its least vertex;
//! - a face: its least vertex, its key, its surface's point at the centre
//!   of its lattice rectangle;
//! - a shell: its least face side (the face's order, `Out` before `In`),
//!   then its kind; a region: its kind, then its outer shell (the
//!   unbounded region, without one, first);
//! - a contact record: its kind, then its member pairs' members in order
//!   (a point on an edge by its parameter from the edge's lesser end).
//!
//! A face's key here is its name ([`FaceKey`]): surfaces' form keys
//! aren't ordered. The ids break only exact ties of these keys. The keys
//! are read off the columns, never by walking links, so they are formed
//! on a body whose topology is broken (the check of such a body reports
//! invariant 1); a link naming no entity counts as missing there. Forming
//! them is charged one unit per entity and per coedge, and the counts the
//! hooks read (the rows naming each loop, face, shell and region) one more
//! per loop, face and shell.
//!
//! Every pass is a [`Work::pass`]: its items run in parallel and are
//! decided in key order, so the failure, the work used and where the
//! budget runs out are the same on any number of threads and in any
//! order the items finish.
//!
//! **Scope** ([`Scope`]). The full check runs every invariant on every
//! item: constructed bodies, loaded bodies, operands. After an operation
//! changed some faces, edges and vertices ([`Changed`]), invariants 1 to
//! 3 and 10 still run everywhere; 4 to 9 only on the changed faces and
//! their neighbours (the faces sharing an edge or a vertex with one),
//! the edges of those faces and the changed edges, the vertices of those
//! edges and the changed vertices, the coedges and loops of those faces,
//! the shells holding one of those faces, the regions of those shells and
//! the contact records with a member among them. A face carried bit for
//! bit has passed already.
//!
//! **Sheets.** A sheet's check is the open-shell variant: every invariant
//! but 8 (the hooks of 1 take the open boundaries from the body's kind).
//!
//! **One invariant left out** ([`Brep::check_except`]): the check with
//! one invariant skipped, which a piece of a body cut out as evidence of
//! that invariant's failure passes. The invariants after it must not rely
//! on it to read the body safely: they meet what it would have refused.
//!
//! **Stages.** A body needing a part of an invariant this build doesn't
//! certify yet (a contact record, a curved trim, a non-adjacent near
//! pair) is refused, never passed ([`Finding::Unbuilt`]). This build has
//! the hooks of invariants 1 to 3 (the [`topo`] module); each later one
//! refuses every face as unbuilt, so only a body without faces passes the
//! whole check.
//!
//! **Outcomes** ([`CheckError::outcome`]): what a failure means depends
//! on whose body it is ([`Role`]): an operand is invalid; a constructed
//! body or an operation's result that fails is the kernel's own defect,
//! but for a seam bound (an accuracy item) or a result's orientation (a
//! signal to its operation's ladder); a constructed face certified too
//! tight to embed is a self-intersection of the construction's inputs, a
//! result face a known rule gap. Running out while a certificate is
//! still open is [`KernelError::OutOfBudget`] whoever's body it is.

use std::ops::Range;

use varde_decide::order::{Along, Coord, EdgeKey, FaceOrderKey, VertKey};

use super::brep::{
    BodyKind, Brep, CoedgeId, ContactKind, EdgeId, FaceId, FaceKey, LoopId, Member, RegionId,
    RegionKind, ShellId, ShellKind, VertId,
};
use crate::infra::budget::{Budget, Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::{Inconsistency, Internal, Invalid, KernelError};

/// An invariant of the check, numbered as in the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Inv {
    /// 1: topology.
    Topology = 1,
    /// 2: bit-exact joins.
    Joins = 2,
    /// 3: domain.
    Domain = 3,
    /// 4: trims.
    Trims = 4,
    /// 5: seams.
    Seams = 5,
    /// 6: regularity and continuity.
    Regularity = 6,
    /// 7: forms and fits.
    Forms = 7,
    /// 8: orientation and regions.
    Orientation = 8,
    /// 9: contacts and embedding.
    Embedding = 9,
    /// 10: names.
    Names = 10,
}

impl Inv {
    /// Every invariant, in order.
    pub const ALL: [Inv; 10] = [
        Inv::Topology,
        Inv::Joins,
        Inv::Domain,
        Inv::Trims,
        Inv::Seams,
        Inv::Regularity,
        Inv::Forms,
        Inv::Orientation,
        Inv::Embedding,
        Inv::Names,
    ];

    /// Its number, 1 to 10.
    pub fn number(self) -> u8 {
        self as u8
    }

    /// The invariant numbered `n`, if any.
    pub fn from_number(n: u8) -> Option<Inv> {
        Inv::ALL.get(usize::from(n).checked_sub(1)?).copied()
    }

    /// Whether a scoped check runs it only near what changed (4 to 9).
    pub fn is_scoped(self) -> bool {
        (4..=9).contains(&self.number())
    }

    /// Whether a sheet's check runs it: every invariant but 8.
    pub fn on_sheets(self) -> bool {
        self != Inv::Orientation
    }

    /// Its stage's name in the budget's profile.
    fn stage(self) -> &'static str {
        [
            "check 1", "check 2", "check 3", "check 4", "check 5", "check 6", "check 7", "check 8",
            "check 9", "check 10",
        ][usize::from(self.number() - 1)]
    }
}

/// The stage of the passes forming the geometric order.
const ORDER_STAGE: &str = "check order";

/// A kind of item an invariant checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    /// The body as a whole.
    Body,
    /// The vertices.
    Vert,
    /// The edges.
    Edge,
    /// The coedges.
    Coedge,
    /// The loops.
    Loop,
    /// The faces.
    Face,
    /// The shells.
    Shell,
    /// The regions.
    Region,
    /// The contact records.
    Contact,
}

/// An item a check failed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    /// The body as a whole.
    Body,
    /// A vertex.
    Vert(VertId),
    /// An edge.
    Edge(EdgeId),
    /// A coedge.
    Coedge(CoedgeId),
    /// A loop.
    Loop(LoopId),
    /// A face.
    Face(FaceId),
    /// A shell.
    Shell(ShellId),
    /// A region.
    Region(RegionId),
    /// A contact record, by its place in the body's records.
    Contact(u32),
}

impl Item {
    /// Its kind.
    pub fn kind(self) -> ItemKind {
        match self {
            Item::Body => ItemKind::Body,
            Item::Vert(_) => ItemKind::Vert,
            Item::Edge(_) => ItemKind::Edge,
            Item::Coedge(_) => ItemKind::Coedge,
            Item::Loop(_) => ItemKind::Loop,
            Item::Face(_) => ItemKind::Face,
            Item::Shell(_) => ItemKind::Shell,
            Item::Region(_) => ItemKind::Region,
            Item::Contact(_) => ItemKind::Contact,
        }
    }
}

/// What a check found wrong at an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Finding {
    /// A certified defect: the item breaks the invariant.
    Defect,
    /// A crossing certified where the body records none: trims meeting
    /// (4), faces crossing (9).
    Crossing,
    /// A face's surroundings certified too tight to embed it: its edge's
    /// window of tube radii is empty (9).
    EmptyWindow,
    /// The item needs a part of the invariant this build doesn't
    /// certify yet.
    Unbuilt,
}

/// A hook's failure at one item: which of its checks, in its own order,
/// and what it found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fault {
    /// The check, by its place in the hook's order.
    pub(crate) check: u32,
    /// What it found.
    pub(crate) finding: Finding,
}

/// The first failure of a check (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckFailure {
    /// The invariant.
    pub inv: Inv,
    /// The item.
    pub item: Item,
    /// The check within the invariant, by its place in the invariant's
    /// order.
    pub check: u32,
    /// What it found.
    pub finding: Finding,
}

/// Why a body fails its check.
#[derive(Clone, Debug, PartialEq)]
pub enum CheckError {
    /// A certified failure, or a part not built yet.
    Failed(CheckFailure),
    /// The budget ran out, or an item reached a fixed limit, with a
    /// certificate still open: always [`KernelError::OutOfBudget`]. `inv`
    /// is the invariant it was certifying, `None` while forming the
    /// order.
    OutOfBudget {
        /// The invariant.
        inv: Option<Inv>,
        /// The error.
        error: KernelError,
    },
}

/// Whose body is checked: what a failure means (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// An operation's operand, from outside the operation.
    Operand,
    /// A body a constructor built.
    Constructed,
    /// An operation's result.
    Result,
}

/// What a failed check means for the operation that ran it.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The operation fails with this error.
    Error(KernelError),
    /// A seam bound of the result over its target: an accuracy item of
    /// the operation, not a failure.
    Accuracy,
    /// The result's orientation is wrong: a signal to the operation's
    /// ladder, which tries again.
    LadderSignal,
}

impl CheckError {
    /// The invariant it failed at; `None` if it ran out while forming
    /// the order.
    pub fn inv(&self) -> Option<Inv> {
        match self {
            CheckError::Failed(f) => Some(f.inv),
            CheckError::OutOfBudget { inv, .. } => *inv,
        }
    }

    /// What it means for a body of role `role` (module docs).
    pub fn outcome(&self, role: Role) -> Outcome {
        let f = match self {
            CheckError::OutOfBudget { error, .. } => return Outcome::Error(error.clone()),
            CheckError::Failed(f) => f,
        };
        let defect = KernelError::Internal(Internal::Inconsistent(Inconsistency::ResultDefect));
        Outcome::Error(match (role, f.finding) {
            (_, Finding::Unbuilt) => unbuilt(),
            (Role::Operand, Finding::Defect) => KernelError::Invalid(Invalid::Check {
                inv: f.inv.number(),
            }),
            (Role::Operand, Finding::Crossing | Finding::EmptyWindow)
            | (Role::Constructed, Finding::EmptyWindow) => {
                KernelError::Invalid(Invalid::SelfIntersection)
            }
            (Role::Result, Finding::Defect) if f.inv == Inv::Seams => return Outcome::Accuracy,
            (Role::Result, Finding::Defect) if f.inv == Inv::Orientation => {
                return Outcome::LadderSignal;
            }
            (Role::Result, Finding::EmptyWindow) => {
                KernelError::Internal(Internal::Inconsistent(Inconsistency::ClusterRuleGap))
            }
            (Role::Constructed, Finding::Defect | Finding::Crossing)
            | (Role::Result, Finding::Defect | Finding::Crossing) => defect,
        })
    }
}

/// The error of a body needing a part of the check this build lacks.
fn unbuilt() -> KernelError {
    #[cfg(feature = "staging")]
    {
        use crate::infra::error::{Stage, Unsupported};
        KernelError::Unsupported(Unsupported::Stage(Stage::CheckStage))
    }
    #[cfg(not(feature = "staging"))]
    {
        KernelError::NotImplemented("a part of the body's check")
    }
}

/// One invariant's checks: the kinds of item it checks, in its order,
/// and the check of one item.
pub(crate) trait Invariant: Sync {
    /// The kinds of item it checks, in its order, each kind one pass.
    fn kinds(&self) -> &[ItemKind];

    /// Checks `item` of `cx`'s body, charging `ledger` before each step:
    /// the first of its checks to fail, in its fixed order, or a fixed
    /// limit reached. It must depend on the item and the body alone, and
    /// not panic on a body whose earlier invariants failed or were left
    /// out.
    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Result<(), Stop<Fault>>;
}

/// The hooks of the ten invariants, in order.
pub(crate) type Invariants<'a> = [&'a dyn Invariant; 10];

/// An invariant whose hook this build doesn't have yet: every face
/// needs it.
struct Unbuilt;

impl Invariant for Unbuilt {
    fn kinds(&self) -> &[ItemKind] {
        &[ItemKind::Face]
    }

    fn check(&self, _: &Cx<'_>, _: Item, ledger: &mut Ledger<'_>) -> Result<(), Stop<Fault>> {
        ledger.charge(Step::Work, 1)?;
        Err(Stop::Failed(Fault {
            check: 0,
            finding: Finding::Unbuilt,
        }))
    }
}

/// The hooks this build checks with.
const BUILT: Invariants<'static> = [
    &topo::Topology,
    &topo::Joins,
    &topo::Domain,
    &Unbuilt,
    &Unbuilt,
    &Unbuilt,
    &Unbuilt,
    &Unbuilt,
    &Unbuilt,
    &Unbuilt,
];

/// What an operation changed: its new and modified faces, and its new
/// edges and vertices. Ids naming no entity of the body are ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Changed {
    /// The new and modified faces.
    pub(crate) faces: Vec<FaceId>,
    /// The new edges.
    pub(crate) edges: Vec<EdgeId>,
    /// The new vertices.
    pub(crate) verts: Vec<VertId>,
}

/// Which items a check covers (module docs).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Scope<'a> {
    /// Every item.
    Full,
    /// Invariants 4 to 9 only near what an operation changed.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the operations check their results so as they land"
        )
    )]
    Changed(&'a Changed),
}

/// What a hook reads: the body, what its coedges say of the entities
/// they meet, and which items the check covers.
pub(crate) struct Cx<'a> {
    body: &'a Brep,
    inc: &'a Incidence,
    scope: Option<&'a InScope>,
}

impl Cx<'_> {
    /// The body.
    pub(crate) fn body(&self) -> &Brep {
        self.body
    }

    /// Whether the scoped invariants (4 to 9) check `item`: every item in
    /// a full check, else those near what changed (module docs).
    pub(crate) fn in_scope(&self, item: Item) -> bool {
        self.scope.is_none_or(|s| s.contains(item))
    }
}

impl Brep {
    /// The full check (module docs) on a budget of its own: every
    /// invariant on every item, the sheet variant on a sheet.
    pub fn check(&self, budget: &Budget) -> Result<(), CheckError> {
        self.check_in(Scope::Full, None, &mut Work::new(budget), 0)
    }

    /// The full check with invariant `inv` left out (module docs).
    pub fn check_except(&self, inv: Inv, budget: &Budget) -> Result<(), CheckError> {
        self.check_in(Scope::Full, Some(inv), &mut Work::new(budget), 0)
    }

    /// The check over `scope`, invariant `except` left out, charged to an
    /// operation's `work` as attempt `rung`.
    pub(crate) fn check_in(
        &self,
        scope: Scope<'_>,
        except: Option<Inv>,
        work: &mut Work,
        rung: u8,
    ) -> Result<(), CheckError> {
        run(self, scope, except, &BUILT, work, rung)
    }
}

/// The check of `body` over `scope` with the hooks `invariants`,
/// invariant `except` left out (module docs).
pub(crate) fn run(
    body: &Brep,
    scope: Scope<'_>,
    except: Option<Inv>,
    invariants: &Invariants<'_>,
    work: &mut Work,
    rung: u8,
) -> Result<(), CheckError> {
    let out = |error| CheckError::OutOfBudget { inv: None, error };
    let inc = Incidence::new(body, work, rung).map_err(out)?;
    let order = Order::new(body, &inc, work, rung).map_err(out)?;
    let in_scope = match scope {
        Scope::Full => None,
        Scope::Changed(changed) => Some(InScope::new(body, &inc, changed)),
    };
    for inv in Inv::ALL {
        if except == Some(inv) || (body.kind() == BodyKind::Sheet && !inv.on_sheets()) {
            continue;
        }
        let covered = if inv.is_scoped() {
            in_scope.as_ref()
        } else {
            None
        };
        let cx = Cx {
            body,
            inc: &inc,
            scope: covered,
        };
        let hook = invariants[usize::from(inv.number() - 1)];
        for &kind in hook.kinds() {
            let items: Vec<Item> = order
                .items(kind)
                .filter(|&item| cx.in_scope(item))
                .collect();
            let pass = Pass {
                stage: inv.stage(),
                rung,
            };
            match work.pass(pass, &items, |&item, ledger| hook.check(&cx, item, ledger)) {
                Ok(_) => {}
                Err(Halt::OutOfBudget(error)) => {
                    return Err(CheckError::OutOfBudget {
                        inv: Some(inv),
                        error,
                    });
                }
                Err(Halt::Failed { item, error }) => {
                    return Err(CheckError::Failed(CheckFailure {
                        inv,
                        item: items[item],
                        check: error.check,
                        finding: error.finding,
                    }));
                }
            }
        }
    }
    Ok(())
}

/// `v[i]`, if there is one.
fn get<T: Copy>(v: &[T], i: usize) -> Option<T> {
    v.get(i).copied()
}

/// A face's key in the order: its name, or none for an id naming no
/// face.
type K = Option<FaceKey>;

/// One pass of `n` items forming part of the order, each charged one
/// unit, giving `f`'s values in id order.
fn pass_ids<R: Send>(
    work: &mut Work,
    rung: u8,
    n: usize,
    f: impl Fn(usize) -> R + Sync + Send,
) -> Result<Vec<R>, KernelError> {
    let ids: Vec<usize> = (0..n).collect();
    let pass = Pass {
        stage: ORDER_STAGE,
        rung,
    };
    work.pass(
        pass,
        &ids,
        |&i, ledger| -> Result<R, Stop<std::convert::Infallible>> {
            ledger.charge(Step::Work, 1)?;
            Ok(f(i))
        },
    )
    .map_err(|halt| match halt {
        Halt::OutOfBudget(error) => error,
        Halt::Failed { error, .. } => match error {},
    })
}

/// What the coedges say of the entities they meet, and how many rows
/// name each loop, face, shell and region, read off the columns alone (a
/// link naming no entity left out), each list sorted and without
/// repeats.
struct Incidence {
    /// Per vertex, the faces of the coedges starting or ending there.
    vert_faces: Vec<Vec<FaceId>>,
    /// Per vertex, its corners: the coedges ending there, in their own
    /// direction.
    vert_corners: Vec<Vec<CoedgeId>>,
    /// Per edge, its coedges.
    edge_coedges: Vec<Vec<CoedgeId>>,
    /// Per face, the vertices of its coedges.
    face_verts: Vec<Vec<VertId>>,
    /// Per face, the edges of its coedges.
    face_edges: Vec<Vec<EdgeId>>,
    /// Per loop, the vertices of its coedges.
    loop_verts: Vec<Vec<VertId>>,
    /// Per loop, the number of coedges naming it.
    loop_sizes: Vec<usize>,
    /// Per face, the number of loops naming it.
    face_loops: Vec<usize>,
    /// Per shell, the number of faces' sides naming it.
    shell_sides: Vec<usize>,
    /// Per region, the number of shells naming it.
    region_shells: Vec<usize>,
}

/// One coedge's incidences: its edge, its two vertices (in its own
/// direction), its face and its loop, each `None` if its id names none.
#[derive(Clone, Copy)]
struct CoedgeInc {
    edge: Option<EdgeId>,
    ends: [Option<VertId>; 2],
    face: Option<FaceId>,
    lp: Option<LoopId>,
}

impl Incidence {
    fn new(body: &Brep, work: &mut Work, rung: u8) -> Result<Incidence, KernelError> {
        let nv = body.vert_count();
        let ne = body.edge_count();
        let nf = body.face_count();
        let nl = body.loop_count();
        let cs = &body.coedges;
        let es = &body.edges;
        let incs = pass_ids(work, rung, body.coedge_count(), |c| {
            let edge = get(&cs.edge, c).filter(|e| e.index() < ne);
            let ends = edge.map_or([None; 2], |e| {
                let vs = get(&es.v_start, e.index()).filter(|v| v.index() < nv);
                let ve = get(&es.v_end, e.index()).filter(|v| v.index() < nv);
                if get(&cs.reversed, c).unwrap_or(false) {
                    [ve, vs]
                } else {
                    [vs, ve]
                }
            });
            CoedgeInc {
                edge,
                ends,
                face: get(&cs.face, c).filter(|f| f.index() < nf),
                lp: get(&cs.lp, c).filter(|l| l.index() < nl),
            }
        })?;
        let ns = body.shell_count();
        let nr = body.region_count();
        let loop_faces = pass_ids(work, rung, nl, |l| {
            get(&body.loops.face, l).filter(|f| f.index() < nf)
        })?;
        let face_shells = pass_ids(work, rung, nf, |f| {
            body.faces.shells[f].map(|s| (s.index() < ns).then_some(s))
        })?;
        let shell_regions = pass_ids(work, rung, ns, |s| {
            get(&body.shells.region, s).filter(|r| r.index() < nr)
        })?;
        let mut inc = Incidence {
            vert_faces: vec![Vec::new(); nv],
            vert_corners: vec![Vec::new(); nv],
            edge_coedges: vec![Vec::new(); ne],
            face_verts: vec![Vec::new(); nf],
            face_edges: vec![Vec::new(); nf],
            loop_verts: vec![Vec::new(); nl],
            loop_sizes: vec![0; nl],
            face_loops: vec![0; nf],
            shell_sides: vec![0; ns],
            region_shells: vec![0; nr],
        };
        for f in loop_faces.into_iter().flatten() {
            inc.face_loops[f.index()] += 1;
        }
        for s in face_shells.into_iter().flatten().flatten() {
            inc.shell_sides[s.index()] += 1;
        }
        for r in shell_regions.into_iter().flatten() {
            inc.region_shells[r.index()] += 1;
        }
        for (c, ci) in incs.iter().enumerate() {
            let ends = ci.ends.iter().flatten();
            // Coedge ids fit in `u32`: the store hands out no more.
            let id = CoedgeId(c as u32);
            if let Some(e) = ci.edge {
                inc.edge_coedges[e.index()].push(id);
            }
            if let Some(v) = ci.ends[1] {
                inc.vert_corners[v.index()].push(id);
            }
            if let Some(f) = ci.face {
                for &v in ends.clone() {
                    inc.vert_faces[v.index()].push(f);
                    inc.face_verts[f.index()].push(v);
                }
                inc.face_edges[f.index()].extend(ci.edge);
            }
            if let Some(l) = ci.lp {
                inc.loop_verts[l.index()].extend(ends);
                inc.loop_sizes[l.index()] += 1;
            }
        }
        inc.vert_faces.iter_mut().for_each(sort_dedup);
        inc.face_edges.iter_mut().for_each(sort_dedup);
        inc.face_verts.iter_mut().for_each(sort_dedup);
        inc.loop_verts.iter_mut().for_each(sort_dedup);
        inc.edge_coedges.iter_mut().for_each(sort_dedup);
        inc.vert_corners.iter_mut().for_each(sort_dedup);
        Ok(inc)
    }
}

fn sort_dedup<T: Ord>(list: &mut Vec<T>) {
    list.sort_unstable();
    list.dedup();
}

/// A face's place in the order (module docs): its order key, `None` for
/// a face without a vertex, then its name.
type FaceRank = (Option<FaceOrderKey<K>>, FaceKey);

/// A contact member's place in the order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MemberRank {
    Vert(VertKey<K>),
    Edge(EdgeKey<K>),
    EdgeAt(EdgeKey<K>, Coord),
    FaceAt(FaceRank, Coord, Coord),
}

/// Each kind's ids in the geometric order (module docs).
struct Order {
    verts: Vec<u32>,
    edges: Vec<u32>,
    coedges: Vec<u32>,
    loops: Vec<u32>,
    faces: Vec<u32>,
    shells: Vec<u32>,
    regions: Vec<u32>,
    contacts: Vec<u32>,
}

/// The ids sorted by their ranks, the ids breaking exact ties.
fn sorted<R: Ord>(ranks: Vec<R>) -> Vec<u32> {
    // Ids fit in `u32`: the store hands out no more.
    let mut pairs: Vec<(R, u32)> = ranks
        .into_iter()
        .enumerate()
        .map(|(i, r)| (r, i as u32))
        .collect();
    varde_decide::order::sort(&mut pairs);
    pairs.into_iter().map(|(_, i)| i).collect()
}

fn shell_kind_rank(k: ShellKind) -> u8 {
    match k {
        ShellKind::Outer => 0,
        ShellKind::Island => 1,
        ShellKind::Open => 2,
    }
}

fn region_kind_rank(k: RegionKind) -> u8 {
    match k {
        RegionKind::Material => 0,
        RegionKind::Empty => 1,
    }
}

fn contact_kind_rank(k: ContactKind) -> u8 {
    match k {
        ContactKind::Edge => 0,
        ContactKind::Vertex => 1,
        ContactKind::Region => 2,
    }
}

/// The middle of a range of tile indices, in global UV.
fn mid(r: &Range<i64>) -> f64 {
    // Within `2⁵³` the indices are exact; the sum and its half are the
    // same on every run either way.
    (r.start as f64 + r.end as f64) * 0.5
}

impl Order {
    fn new(body: &Brep, inc: &Incidence, work: &mut Work, rung: u8) -> Result<Order, KernelError> {
        let nf = body.face_count();
        let fkey = |f: FaceId| (f.index() < nf).then(|| *body.key(f));
        let vkeys = pass_ids(work, rung, body.vert_count(), |v| {
            let faces = inc.vert_faces[v].iter().map(|&f| fkey(f));
            // A position that isn't a number (only a broken store holds
            // one) is ordered as infinite there.
            let pos = body.verts.pos[v].map(|x| if x.is_nan() { f64::INFINITY } else { x });
            VertKey::new(pos, faces).expect("no NaN is left")
        })?;
        let edge_ranks = pass_ids(work, rung, body.edge_count(), |e| {
            edge_rank(body, inc, &vkeys, &fkey, EdgeId(e as u32))
        })?;
        let face_ranks = pass_ids(work, rung, nf, |f| {
            let f = FaceId(f as u32);
            let key = *body.key(f);
            let verts = inc.face_verts[f.index()]
                .iter()
                .map(|v| vkeys[v.index()].clone());
            let order = FaceOrderKey::new(verts, Some(key), centre(body, f)).ok();
            (order, key)
        })?;
        let coedge_ranks = pass_ids(work, rung, body.coedge_count(), |c| {
            let cs = &body.coedges;
            let e = get(&cs.edge, c).and_then(|e| edge_ranks.get(e.index())?.as_ref());
            e.map(|(key, along)| {
                let reversed = get(&cs.reversed, c).unwrap_or(false);
                let from_lesser = reversed == (*along == Along::Reversed);
                let face = get(&cs.face, c).and_then(fkey);
                (key.clone(), face, from_lesser)
            })
        })?;
        let loop_ranks = pass_ids(work, rung, body.loop_count(), |l| {
            let ls = &body.loops;
            let face = get(&ls.face, l).and_then(fkey);
            let outer = get(&ls.is_outer, l).unwrap_or(false);
            let least = inc.loop_verts[l].iter().map(|v| &vkeys[v.index()]).min();
            (face, !outer, least.cloned())
        })?;
        let shell_ranks = pass_ids(work, rung, body.shell_count(), |s| {
            let ss = &body.shells;
            let side = ss.sides[s]
                .iter()
                .filter(|(f, _)| f.index() < nf)
                .map(|&(f, side)| (&face_ranks[f.index()], side))
                .min();
            let kind = shell_kind_rank(ss.kind[s]);
            (side.map(|(r, side)| (r.clone(), side)), kind)
        })?;
        let shell_rank = |s: ShellId| shell_ranks.get(s.index()).cloned();
        let region_ranks = pass_ids(work, rung, body.region_count(), |r| {
            let rs = &body.regions;
            let outer = get(&rs.outer, r).and_then(shell_rank);
            (region_kind_rank(rs.kind[r]), outer)
        })?;
        let contact_ranks = pass_ids(work, rung, body.contacts.len(), |k| {
            let c = &body.contacts[k];
            let member = |m: &Member| member_rank(m, &vkeys, &edge_ranks, &face_ranks);
            let mut pairs: Vec<[Option<MemberRank>; 2]> = c
                .members
                .iter()
                .map(|[a, b]| [member(a), member(b)])
                .collect();
            pairs.sort_unstable();
            (contact_kind_rank(c.kind), pairs)
        })?;
        Ok(Order {
            verts: sorted(vkeys),
            edges: sorted(edge_ranks.into_iter().map(|r| r.map(|(k, _)| k)).collect()),
            coedges: sorted(coedge_ranks),
            loops: sorted(loop_ranks),
            faces: sorted(face_ranks),
            shells: sorted(shell_ranks),
            regions: sorted(region_ranks),
            contacts: sorted(contact_ranks),
        })
    }

    /// The items of kind `kind`, in order.
    fn items(&self, kind: ItemKind) -> Box<dyn Iterator<Item = Item> + '_> {
        fn ids(v: &[u32]) -> std::iter::Copied<std::slice::Iter<'_, u32>> {
            v.iter().copied()
        }
        match kind {
            ItemKind::Body => Box::new(std::iter::once(Item::Body)),
            ItemKind::Vert => Box::new(ids(&self.verts).map(|i| Item::Vert(VertId(i)))),
            ItemKind::Edge => Box::new(ids(&self.edges).map(|i| Item::Edge(EdgeId(i)))),
            ItemKind::Coedge => Box::new(ids(&self.coedges).map(|i| Item::Coedge(CoedgeId(i)))),
            ItemKind::Loop => Box::new(ids(&self.loops).map(|i| Item::Loop(LoopId(i)))),
            ItemKind::Face => Box::new(ids(&self.faces).map(|i| Item::Face(FaceId(i)))),
            ItemKind::Shell => Box::new(ids(&self.shells).map(|i| Item::Shell(ShellId(i)))),
            ItemKind::Region => Box::new(ids(&self.regions).map(|i| Item::Region(RegionId(i)))),
            ItemKind::Contact => Box::new(ids(&self.contacts).map(Item::Contact)),
        }
    }
}

/// An edge's place in the order, and which way its stored parameter runs
/// from its lesser end; `None` if a vertex id names no vertex.
fn edge_rank(
    body: &Brep,
    inc: &Incidence,
    vkeys: &[VertKey<K>],
    fkey: &impl Fn(FaceId) -> K,
    e: EdgeId,
) -> Option<(EdgeKey<K>, Along)> {
    let [vs, ve] = body.ends(e);
    let (s, t) = (vkeys.get(vs.index())?, vkeys.get(ve.index())?);
    let [t0, t1] = body.t_range(e);
    let mid = (body.curves3.get(body.edge_curve_id(e).index()))
        .and_then(|c| c.point_at((t0 + t1) * 0.5))
        .filter(|p| p.iter().all(|x| !x.is_nan()))
        .unwrap_or_else(|| s.pos().get());
    let mut faces: Vec<K> = inc.edge_coedges[e.index()]
        .iter()
        .filter_map(|&c| get(&body.coedges.face, c.index()))
        .map(fkey)
        .collect();
    faces.sort_unstable();
    let faces = [get(&faces, 0).flatten(), get(&faces, 1).flatten()];
    let key = EdgeKey::new([s.clone(), t.clone()], mid, faces).expect("a NaN is replaced");
    Some((key, Along::of(s, t)))
}

/// A face's surface at the centre of its lattice rectangle, or its
/// lattice's origin where it has no point there.
fn centre(body: &Brep, f: FaceId) -> [f64; 3] {
    let Some(s) = body.surfaces.surface.get(body.face_surface_id(f).index()) else {
        return [0.0; 3];
    };
    let r = s.rect();
    s.eval([mid(&r.u), mid(&r.v)])
        .map(|p| p.map(|x| x.v))
        .filter(|p| p.iter().all(|x| !x.is_nan()))
        .unwrap_or_else(|| s.lattice().origin())
}

/// A contact member's place in the order, `None` if it names no entity
/// or its parameter is a NaN.
fn member_rank(
    m: &Member,
    vkeys: &[VertKey<K>],
    edges: &[Option<(EdgeKey<K>, Along)>],
    faces: &[FaceRank],
) -> Option<MemberRank> {
    Some(match *m {
        Member::Vert(v) => MemberRank::Vert(vkeys.get(v.index())?.clone()),
        Member::Edge(e) => MemberRank::Edge(edges.get(e.index())?.as_ref()?.0.clone()),
        Member::EdgeAt { edge, t } => {
            let (key, along) = edges.get(edge.index())?.as_ref()?;
            MemberRank::EdgeAt(key.clone(), along.param(t).ok()?)
        }
        Member::FaceAt { face, uv } => MemberRank::FaceAt(
            faces.get(face.index())?.clone(),
            Coord::new(uv[0]).ok()?,
            Coord::new(uv[1]).ok()?,
        ),
    })
}

/// Which items a scoped check covers (module docs), per kind.
struct InScope {
    verts: Vec<bool>,
    edges: Vec<bool>,
    coedges: Vec<bool>,
    loops: Vec<bool>,
    faces: Vec<bool>,
    shells: Vec<bool>,
    regions: Vec<bool>,
    contacts: Vec<bool>,
}

impl InScope {
    fn new(body: &Brep, inc: &Incidence, changed: &Changed) -> InScope {
        let nf = body.face_count();
        let ne = body.edge_count();
        let nv = body.vert_count();
        let mut faces = vec![false; nf];
        for &f in changed.faces.iter().filter(|f| f.index() < nf) {
            faces[f.index()] = true;
            let by_edge = inc.face_edges[f.index()]
                .iter()
                .flat_map(|e| &inc.edge_coedges[e.index()])
                .filter_map(|c| get(&body.coedges.face, c.index()));
            let by_vert = inc.face_verts[f.index()]
                .iter()
                .flat_map(|v| inc.vert_faces[v.index()].iter().copied());
            for g in by_edge.chain(by_vert).filter(|g| g.index() < nf) {
                faces[g.index()] = true;
            }
        }
        let mut edges = vec![false; ne];
        for f in (0..nf).filter(|&f| faces[f]) {
            for e in &inc.face_edges[f] {
                edges[e.index()] = true;
            }
        }
        for e in changed.edges.iter().filter(|e| e.index() < ne) {
            edges[e.index()] = true;
        }
        let mut verts = vec![false; nv];
        for e in (0..ne).filter(|&e| edges[e]) {
            for v in body.ends(EdgeId(e as u32)) {
                if let Some(x) = verts.get_mut(v.index()) {
                    *x = true;
                }
            }
        }
        for v in changed.verts.iter().filter(|v| v.index() < nv) {
            verts[v.index()] = true;
        }
        let face_in = |f: Option<FaceId>| f.is_some_and(|f| get(&faces, f.index()) == Some(true));
        let coedges = body
            .coedges
            .face
            .iter()
            .map(|&f| face_in(Some(f)))
            .collect();
        let loops = body.loops.face.iter().map(|&f| face_in(Some(f))).collect();
        let shells: Vec<bool> = (body.shells.sides.iter())
            .map(|sides| sides.iter().any(|&(f, _)| face_in(Some(f))))
            .collect();
        let shell_in = |s: ShellId| get(&shells, s.index()) == Some(true);
        let regions = (0..body.region_count())
            .map(|r| {
                let rs = &body.regions;
                shell_in(rs.outer[r]) || rs.islands[r].iter().any(|&s| shell_in(s))
            })
            .collect();
        let member_in = |m: &Member| match *m {
            Member::Vert(v) => get(&verts, v.index()) == Some(true),
            Member::Edge(e) | Member::EdgeAt { edge: e, .. } => {
                get(&edges, e.index()) == Some(true)
            }
            Member::FaceAt { face, .. } => face_in(Some(face)),
        };
        let contacts = (body.contacts.iter())
            .map(|c| c.members.iter().flatten().any(member_in))
            .collect();
        InScope {
            verts,
            edges,
            coedges,
            loops,
            faces,
            shells,
            regions,
            contacts,
        }
    }

    fn contains(&self, item: Item) -> bool {
        let at = |v: &[bool], i: u32| get(v, i as usize) == Some(true);
        match item {
            Item::Body => true,
            Item::Vert(v) => at(&self.verts, v.0),
            Item::Edge(e) => at(&self.edges, e.0),
            Item::Coedge(c) => at(&self.coedges, c.0),
            Item::Loop(l) => at(&self.loops, l.0),
            Item::Face(f) => at(&self.faces, f.0),
            Item::Shell(s) => at(&self.shells, s.0),
            Item::Region(r) => at(&self.regions, r.0),
            Item::Contact(k) => at(&self.contacts, k),
        }
    }
}

mod topo;

#[cfg(test)]
mod tests;
