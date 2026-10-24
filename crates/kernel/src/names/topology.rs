//! User topology: the faces, edges and corners a user sees and refers
//! to, derived from a body's keys and edge fields, never stored. It is
//! the one layer whose names are promised to stay the same when a model
//! is rebuilt with other dimensions, another tolerance, other tiles or
//! other ids.
//!
//! **Transparent edges.** An edge between two faces is *transparent*,
//! inside what a user sees as one face, when it is of the same form
//! ([`EdgeKind::SameForm`]), tagged a seam, or has one key on both sides
//! and either one form key, a tangent or curvature-continuous join, or a
//! tangency cluster's origin (a one-key rim, whatever its continuity),
//! unless it is a contact edge.
//!
//! **Contact edges** join faces of *different* keys where material only
//! touched, and are never transparent: a tangency cluster's rim edge (its
//! origin a near or contact cluster, never a moved join, whose clusters
//! only moved its ends), an edge some contact record names as a member,
//! or a same-form edge whose faces only touched
//! ([`EdgeKind::SameForm`]`{touch}`). So coplanar faces of different keys
//! that only touch (tiles edge to edge, a block stood flush on a plate)
//! stay separate faces, while a co-oriented overlap, joined by an
//! ordinary same-form edge, is one.
//!
//! **What is derived** ([`Topology`]):
//! - a [`Region`]: the faces connected only across transparent edges.
//!   Its **primary** key is the least of its faces' keys in the leaf
//!   order, the others its **aliases**. A key may be one region's primary
//!   and another's alias (one half of a grooved face joined to a coplanar
//!   face of a key earlier in the leaf order);
//! - a [`Chain`]: a maximal path of edges that are not transparent,
//!   between the same two regions, through vertices where exactly two
//!   such edges meet; a sheet's open boundary makes chains of one region;
//! - a [`Corner`]: a vertex where three or more regions meet, or a
//!   tagged pole or apex;
//! - a [`FlushGroup`]: the regions connected across touching same-form
//!   edges, all on one surface form. The kernel reports them as merged,
//!   so an application may show one face, while references, names and
//!   history stay per region.
//!
//! **Strong and weak** ([`Strength`]). Regions are strong. A chain is
//! weak when it holds a contact edge or lies between two regions of one
//! primary key; a corner when a contact edge ends there (a corner of a
//! tagged pole or apex, and of three regions, is strong otherwise). A
//! rebuild warns on references to weak entities.
//!
//! **Ids.** The regions, chains, corners and groups are numbered in the
//! order of their names, ids breaking ties (two halves of a grooved face
//! share a primary). The numbers, like ids, only identify entities within
//! one body; the sets and names are the same whatever the body's ids.
//!
//! **Work.** Deriving is charged one unit per face, edge, vertex and
//! coedge, and one per contact record's member.
//!
//! **Reading a broken body.** A link naming no entity is passed over, so
//! a body whose topology the check refuses is still read without
//! panicking.

use std::convert::Infallible;

use crate::infra::budget::{Budget, Exhausted, Halt, Ledger, Pass, Step, Stop, Work};
use crate::infra::error::KernelError;
use crate::topo::brep::{
    Brep, CoedgeId, Continuity, EdgeId, EdgeKind, FaceId, Member, Origin, PlainOrigin, VertId,
};

use super::derive::{Between, Meet};
use super::keys::{EdgeTag, FaceKey, VertTag};

#[cfg(test)]
pub(crate) mod tests;

/// Whether references to an entity are kept through rebuilds (module
/// docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Strength {
    /// Kept: a rebuild resolves references to it quietly.
    Strong,
    /// Present or not depending on touching and tolerance: a rebuild
    /// warns on references to it.
    Weak,
}

/// What an edge is to the user topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeClass {
    /// Inside a region.
    Transparent,
    /// Between regions, the faces not merely touching.
    Crease,
    /// Between faces of different keys that only touched: never
    /// transparent.
    Contact,
    /// A sheet's open boundary: one face.
    Boundary,
    /// Part of no chain and no region's join: a pole's or apex's side,
    /// or an edge no coedge uses.
    Apart,
}

impl EdgeClass {
    /// Whether the edge belongs to a chain.
    pub fn is_chain(self) -> bool {
        matches!(
            self,
            EdgeClass::Crease | EdgeClass::Contact | EdgeClass::Boundary
        )
    }
}

/// A face as users see it: faces joined across transparent edges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    /// The least of its faces' keys in the leaf order: its name.
    pub primary: FaceKey,
    /// Its faces' other keys, ascending.
    pub aliases: Vec<FaceKey>,
    /// Its faces, ascending.
    pub faces: Vec<FaceId>,
    /// Its flush group.
    pub flush: u32,
}

impl Region {
    /// Whether `key` names it: its primary or one of its aliases.
    pub fn named(&self, key: &FaceKey) -> bool {
        self.primary == *key || self.aliases.binary_search(key).is_ok()
    }
}

/// An edge as users see it: a maximal path of edges between the same
/// two regions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    /// The regions either side, ascending; one region twice on a sheet's
    /// open boundary, or where a crease of one key separates a region
    /// from itself.
    pub regions: [u32; 2],
    /// Whether it is a sheet's open boundary.
    pub boundary: bool,
    /// Its edges, ascending.
    pub edges: Vec<EdgeId>,
    /// Whether it closes on itself rather than ending at a vertex where
    /// other chains meet.
    pub closed: bool,
    /// Whether one of its edges is a contact edge.
    pub contact: bool,
    /// Whether references to it are kept.
    pub strength: Strength,
}

/// A corner as users see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Corner {
    /// The vertex.
    pub vert: VertId,
    /// The regions meeting there, ascending.
    pub regions: Vec<u32>,
    /// The vertex's tag.
    pub tag: Option<VertTag>,
    /// Whether references to it are kept.
    pub strength: Strength,
}

/// Regions on one surface form that only touched, reported as merged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlushGroup {
    /// The regions, in the leaf order of their primary keys.
    pub regions: Vec<u32>,
}

/// A body's regions, chains, corners and flush groups (module docs).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Topology {
    regions: Vec<Region>,
    chains: Vec<Chain>,
    corners: Vec<Corner>,
    flush: Vec<FlushGroup>,
    /// Per face, its region.
    region_of: Vec<u32>,
    /// Per edge, its class.
    classes: Vec<EdgeClass>,
    /// Per edge, its chain.
    chain_of: Vec<u32>,
}

/// No region or chain.
const NONE: u32 = u32::MAX;

/// The stage the derivation's work is profiled under.
const STAGE: &str = "user topology";

impl Topology {
    /// The user topology of `body` (module docs), on a budget of its own.
    pub fn new(body: &Brep, budget: &Budget) -> Result<Topology, KernelError> {
        let pass = Pass {
            stage: STAGE,
            rung: 0,
        };
        let mut work = Work::new(budget);
        let out = work.pass(pass, &[()], |(), ledger| -> Result<_, Stop<Infallible>> {
            let contact = contact_edges(body, ledger)?;
            Ok(Topology::derive(body, &contact, ledger)?)
        });
        match out {
            Ok(mut t) => Ok(t.pop().unwrap_or_default()),
            Err(Halt::OutOfBudget(e)) => Err(e),
            Err(Halt::Failed { error, .. }) => match error {},
        }
    }

    /// The user topology of `body`, `contact[e]` telling whether a
    /// contact record names edge `e`, charged to `ledger`.
    pub(crate) fn derive(
        body: &Brep,
        contact: &[bool],
        ledger: &mut Ledger<'_>,
    ) -> Result<Topology, Exhausted> {
        let r = Reader::new(body);
        let units = [r.nf, r.ne, r.nv, body.coedge_count()]
            .iter()
            .fold(0u64, |s, &n| s.saturating_add(n as u64));
        ledger.charge(Step::Work, units)?;
        let ends: Vec<Option<Ends>> = (0..r.ne).map(|e| r.edge(EdgeId(e as u32))).collect();
        let classes: Vec<EdgeClass> = (0..r.ne)
            .map(|e| {
                class(
                    body,
                    EdgeId(e as u32),
                    ends[e],
                    contact.get(e) == Some(&true),
                )
            })
            .collect();
        let (regions, region_of) = regions(body, &r, &ends, &classes);
        let mut t = Topology {
            regions,
            chains: Vec::new(),
            corners: Vec::new(),
            flush: Vec::new(),
            region_of,
            classes,
            chain_of: vec![NONE; r.ne],
        };
        t.find_chains(body, &r, &ends);
        t.find_corners(body, &r);
        t.find_flush(&ends);
        Ok(t)
    }

    /// The regions, in the order of their primary keys.
    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    /// The chains, in the order of their regions.
    pub fn chains(&self) -> &[Chain] {
        &self.chains
    }

    /// The corners, in the order of their names.
    pub fn corners(&self) -> &[Corner] {
        &self.corners
    }

    /// The flush groups, in the order of their first regions.
    pub fn flush_groups(&self) -> &[FlushGroup] {
        &self.flush
    }

    /// Face `f`'s region.
    pub fn region_of(&self, f: FaceId) -> Option<u32> {
        self.region_of.get(f.index()).copied()
    }

    /// Edge `e`'s class.
    pub fn edge_class(&self, e: EdgeId) -> Option<EdgeClass> {
        self.classes.get(e.index()).copied()
    }

    /// Edge `e`'s chain, if it is in one.
    pub fn chain_of(&self, e: EdgeId) -> Option<u32> {
        self.chain_of.get(e.index()).copied().filter(|&c| c != NONE)
    }

    /// A chain's name: between its regions' primary keys.
    pub fn chain_name(&self, c: &Chain) -> Between {
        let [a, b] = c.regions.map(|r| self.regions[r as usize].primary);
        Between::new(a, b)
    }

    /// A corner's name: the meet of its regions' primary keys.
    pub fn corner_name(&self, c: &Corner) -> Meet {
        Meet::new(c.regions.iter().map(|&r| self.regions[r as usize].primary))
    }

    fn find_chains(&mut self, body: &Brep, r: &Reader<'_>, ends: &[Option<Ends>]) {
        let ne = r.ne;
        let side = |e: usize| -> Option<([u32; 2], bool)> {
            if !self.classes[e].is_chain() {
                return None;
            }
            let faces = ends[e]?.faces;
            let rs = faces.map(|f| f.map_or(NONE, |f| self.region_of[f.index()]));
            let boundary = self.classes[e] == EdgeClass::Boundary;
            let rs = if boundary { [rs[0]; 2] } else { rs };
            Some(([rs[0].min(rs[1]), rs[0].max(rs[1])], boundary))
        };
        let sides: Vec<Option<([u32; 2], bool)>> = (0..ne).map(side).collect();
        // Per vertex, the chain edges ending there (a closed edge twice).
        let mut at: Vec<(u32, u32)> = Vec::new();
        for e in (0..ne).filter(|&e| sides[e].is_some()) {
            for v in body.ends(EdgeId(e as u32)) {
                if v.index() < r.nv {
                    at.push((v.0, e as u32));
                }
            }
        }
        at.sort_unstable();
        let mut first: Vec<u32> = (0..ne as u32).collect();
        let mut through = vec![false; r.nv];
        for group in at.chunk_by(|a, b| a.0 == b.0) {
            if let [(v, e1), (_, e2)] = *group
                && sides[e1 as usize] == sides[e2 as usize]
            {
                through[v as usize] = true;
                join(&mut first, e1, e2);
            }
        }
        let mut members: Vec<(u32, u32)> = (0..ne as u32)
            .filter(|&e| sides[e as usize].is_some())
            .map(|e| (root(&mut first, e), e))
            .collect();
        members.sort_unstable();
        let mut chains: Vec<Chain> = members
            .chunk_by(|a, b| a.0 == b.0)
            .map(|group| {
                let edges: Vec<EdgeId> = group.iter().map(|&(_, e)| EdgeId(e)).collect();
                let (regions, boundary) = sides[edges[0].index()].expect("a chain edge");
                let closed = edges.iter().all(|&e| {
                    body.ends(e)
                        .iter()
                        .all(|v| through.get(v.index()) == Some(&true))
                });
                let contact = edges
                    .iter()
                    .any(|&e| self.classes[e.index()] == EdgeClass::Contact);
                let keys = regions.map(|x| self.regions.get(x as usize).map(|g| g.primary));
                let one_key = !boundary && keys[0] == keys[1];
                Chain {
                    regions,
                    boundary,
                    edges,
                    closed,
                    contact,
                    strength: if contact || one_key {
                        Strength::Weak
                    } else {
                        Strength::Strong
                    },
                }
            })
            .collect();
        let rank = |c: &Chain| {
            let keys = c
                .regions
                .map(|x| self.regions.get(x as usize).map(|g| g.primary));
            (keys, c.boundary, c.edges[0])
        };
        chains.sort_by_cached_key(rank);
        for (i, c) in chains.iter().enumerate() {
            for e in &c.edges {
                self.chain_of[e.index()] = i as u32;
            }
        }
        self.chains = chains;
    }

    fn find_corners(&mut self, body: &Brep, r: &Reader<'_>) {
        // Per vertex, the regions of the faces of its coedges.
        let mut at: Vec<Vec<u32>> = vec![Vec::new(); r.nv];
        for c in 0..body.coedge_count() {
            let c = CoedgeId(c as u32);
            let (Some(f), Some(ends)) = (r.coedge_face(c), r.coedge_ends(c)) else {
                continue;
            };
            for v in ends {
                at[v.index()].push(self.region_of[f.index()]);
            }
        }
        // Per vertex, whether a contact edge ends there.
        let mut touched = vec![false; r.nv];
        for (e, class) in self.classes.iter().enumerate() {
            if *class == EdgeClass::Contact {
                for v in body.ends(EdgeId(e as u32)) {
                    if let Some(t) = touched.get_mut(v.index()) {
                        *t = true;
                    }
                }
            }
        }
        let mut corners: Vec<Corner> = Vec::new();
        for (v, mut regions) in at.into_iter().enumerate() {
            regions.sort_unstable();
            regions.dedup();
            let vert = VertId(v as u32);
            let tag = body.vert_tag(vert);
            if regions.len() >= 3 || matches!(tag, Some(VertTag::Pole(_) | VertTag::Apex)) {
                corners.push(Corner {
                    vert,
                    regions,
                    tag,
                    strength: if touched[v] {
                        Strength::Weak
                    } else {
                        Strength::Strong
                    },
                });
            }
        }
        corners.sort_by_cached_key(|c| (self.corner_name(c), c.tag, c.vert));
        self.corners = corners;
    }

    fn find_flush(&mut self, ends: &[Option<Ends>]) {
        let n = self.regions.len();
        let mut first: Vec<u32> = (0..n as u32).collect();
        for (e, class) in self.classes.iter().enumerate() {
            if *class != EdgeClass::Contact {
                continue;
            }
            if let Some(Ends {
                faces: [Some(f), Some(g)],
                touch: true,
            }) = ends[e]
            {
                join(
                    &mut first,
                    self.region_of[f.index()],
                    self.region_of[g.index()],
                );
            }
        }
        let mut members: Vec<(u32, (FaceKey, u32))> = (0..n as u32)
            .map(|x| (root(&mut first, x), (self.regions[x as usize].primary, x)))
            .collect();
        members.sort_unstable();
        let mut groups: Vec<FlushGroup> = members
            .chunk_by(|a, b| a.0 == b.0)
            .map(|g| {
                let mut regions: Vec<(FaceKey, u32)> = g.iter().map(|&(_, m)| m).collect();
                regions.sort_unstable();
                FlushGroup {
                    regions: regions.into_iter().map(|(_, x)| x).collect(),
                }
            })
            .collect();
        groups.sort_by_key(|g| g.regions[0]);
        for (i, g) in groups.iter().enumerate() {
            for &x in &g.regions {
                self.regions[x as usize].flush = i as u32;
            }
        }
        self.flush = groups;
    }
}

/// Which edges contact records name as members (`Member::Edge`), charged
/// one unit per member.
pub(crate) fn contact_edges(body: &Brep, ledger: &mut Ledger<'_>) -> Result<Vec<bool>, Exhausted> {
    let mut out = vec![false; body.edge_count()];
    for c in body.contacts() {
        ledger.charge(Step::Work, c.members.len() as u64)?;
        for m in c.members.iter().flatten() {
            if let Member::Edge(e) = *m
                && let Some(x) = out.get_mut(e.index())
            {
                *x = true;
            }
        }
    }
    Ok(out)
}

/// An edge's faces (the second `None` on a one-coedge edge) and whether
/// it is a touching same-form edge.
#[derive(Clone, Copy, Debug)]
struct Ends {
    faces: [Option<FaceId>; 2],
    touch: bool,
}

/// Reads a body's links, passing over those naming no entity.
struct Reader<'a> {
    body: &'a Brep,
    nf: usize,
    ne: usize,
    nv: usize,
    nc: usize,
}

impl<'a> Reader<'a> {
    fn new(body: &'a Brep) -> Reader<'a> {
        Reader {
            body,
            nf: body.face_count(),
            ne: body.edge_count(),
            nv: body.vert_count(),
            nc: body.coedge_count(),
        }
    }

    fn coedge(&self, c: CoedgeId) -> Option<CoedgeId> {
        (c.index() < self.nc).then_some(c)
    }

    fn coedge_face(&self, c: CoedgeId) -> Option<FaceId> {
        let f = self.body.face(self.coedge(c)?);
        (f.index() < self.nf).then_some(f)
    }

    fn coedge_ends(&self, c: CoedgeId) -> Option<[VertId; 2]> {
        let e = self.body.edge(self.coedge(c)?);
        if e.index() >= self.ne {
            return None;
        }
        let ends = self.body.ends(e);
        ends.iter().all(|v| v.index() < self.nv).then_some(ends)
    }

    /// Edge `e`'s faces, `None` for an edge no coedge uses.
    fn edge(&self, e: EdgeId) -> Option<Ends> {
        let c = self.coedge(self.body.edge_coedge(e))?;
        let p = self.coedge(self.body.pair(c));
        let faces = [self.coedge_face(c), p.and_then(|p| self.coedge_face(p))];
        Some(Ends {
            faces,
            touch: self.body.edge_kind(e).is_touch(),
        })
    }
}

/// Edge `e`'s class (module docs), `member` whether a contact record
/// names it.
fn class(body: &Brep, e: EdgeId, ends: Option<Ends>, member: bool) -> EdgeClass {
    let kind = body.edge_kind(e);
    let Some(Ends { faces, .. }) = ends else {
        return EdgeClass::Apart;
    };
    let (f, g) = match faces {
        [Some(f), Some(g)] => (f, g),
        [Some(_), None] if kind != EdgeKind::Singular => return EdgeClass::Boundary,
        _ => return EdgeClass::Apart,
    };
    let (kf, kg) = (body.key(f), body.key(g));
    let origin = body.origin(e);
    let rim = matches!(origin, Origin::Plain(PlainOrigin::Cluster(_)));
    if kf != kg && (rim || member || kind.is_touch()) {
        return EdgeClass::Contact;
    }
    let one_key = kf == kg
        && (one_form(body, f, g)
            || matches!(
                body.cont(e),
                Continuity::Tangent { .. } | Continuity::Curvature
            )
            || matches!(origin.prior(), PlainOrigin::Cluster(_)));
    let seam = body.edge_tag(e) == Some(EdgeTag::Seam);
    if matches!(kind, EdgeKind::SameForm { .. }) || seam || one_key {
        EdgeClass::Transparent
    } else {
        EdgeClass::Crease
    }
}

/// Whether faces `f` and `g` lie on surfaces of one form key, bit for
/// bit; not where a surface id names none.
fn one_form(body: &Brep, f: FaceId, g: FaceId) -> bool {
    let n = body.surface_count();
    let (s, t) = (body.face_surface_id(f), body.face_surface_id(g));
    s.index() < n
        && t.index() < n
        && body.surface(s).lattice().key() == body.surface(t).lattice().key()
}

/// The regions, numbered in the order of their primary keys (their least
/// faces breaking ties), and each face's region.
fn regions(
    body: &Brep,
    r: &Reader<'_>,
    ends: &[Option<Ends>],
    classes: &[EdgeClass],
) -> (Vec<Region>, Vec<u32>) {
    let mut first: Vec<u32> = (0..r.nf as u32).collect();
    for (e, class) in classes.iter().enumerate() {
        if let (
            EdgeClass::Transparent,
            Some(Ends {
                faces: [Some(f), Some(g)],
                ..
            }),
        ) = (class, ends[e])
        {
            join(&mut first, f.0, g.0);
        }
    }
    let mut members: Vec<(u32, u32)> = (0..r.nf as u32).map(|f| (root(&mut first, f), f)).collect();
    members.sort_unstable();
    let mut regions: Vec<Region> = members
        .chunk_by(|a, b| a.0 == b.0)
        .map(|group| {
            let faces: Vec<FaceId> = group.iter().map(|&(_, f)| FaceId(f)).collect();
            let mut keys: Vec<FaceKey> = faces.iter().map(|&f| *body.key(f)).collect();
            keys.sort_unstable();
            keys.dedup();
            let primary = keys.remove(0);
            Region {
                primary,
                aliases: keys,
                faces,
                flush: 0,
            }
        })
        .collect();
    regions.sort_by(|a, b| (a.primary, a.faces[0]).cmp(&(b.primary, b.faces[0])));
    let mut region_of = vec![NONE; r.nf];
    for (i, g) in regions.iter().enumerate() {
        for f in &g.faces {
            region_of[f.index()] = i as u32;
        }
    }
    (regions, region_of)
}

/// The least member of `c`'s set in the union-find `first`, each set's
/// members pointing towards it.
fn root(first: &mut [u32], c: u32) -> u32 {
    let mut r = c;
    while first[r as usize] != r {
        r = first[r as usize];
    }
    let mut at = c;
    while first[at as usize] != r {
        let next = first[at as usize];
        first[at as usize] = r;
        at = next;
    }
    r
}

/// Joins the sets of `a` and `b` under the lesser of their least members.
fn join(first: &mut [u32], a: u32, b: u32) {
    let (ra, rb) = (root(first, a), root(first, b));
    let (low, high) = (ra.min(rb), ra.max(rb));
    first[high as usize] = low;
}
