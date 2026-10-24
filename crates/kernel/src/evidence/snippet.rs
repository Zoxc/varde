//! The geometry a failure is about: small sheet bodies cut out of its
//! leaf operands ([`SheetSnippet`], [`Piece`]), in the leaves'
//! coordinates, drawable.
//!
//! **A piece** is a set of faces of one leaf copied into a sheet bit for
//! bit: the faces, their loops and coedges, the edges and vertices those
//! use, the surfaces, curves and p-curves they name, the contact records
//! whose members all come along, and the body's tolerances and name
//! tables. Ids are new (in the order of the leaf's, so an edge stays
//! canonical), and each piece keeps the leaf ids of its faces, edges and
//! vertices for whoever replays the failure on the leaf. A coedge whose
//! pair stays behind becomes an open boundary of the sheet; its edge's
//! fields derived from the two faces (the same-form kind, the seam tag,
//! the certified join) no longer hold and are reset to an ordinary
//! edge's. The faces' sides form two open shells per connected part (the
//! `Out` sides, then the `In` sides), rebuilt, not copied.
//!
//! **What it passes.** A piece cut for a failing invariant carries the
//! defect, so it passes the check with that invariant left out
//! ([`Brep::check_except`]); any other piece passes the sheet's full
//! check ([`SheetSnippet::check`]). The raw bits of the faces' tile nets,
//! which defects of invariants 2, 6 and 7 are about, come along as they
//! are stored.
//!
//! The cut reads the leaf guarded: a link naming nothing is dropped (or
//! the piece refused, where a row the piece needs isn't there), never a
//! panic, so a broken operand's evidence can be cut out of it.
//!
//! **Bounds.** A snippet holds at most [`MAX_SNIPPET`] faces over its
//! pieces; past them, and past the evidence allowance (a unit per face,
//! loop and coedge copied), faces are left out and counted in its
//! `truncated`.

use std::collections::BTreeMap;

use varde_decide::num::storage::OpStart;

use super::Allowance;
use super::inputref::Leaf;
use crate::infra::budget::Budget;
use crate::infra::ops::{InstanceId, Status};
use crate::topo::brep::{
    BodyKind, Brep, ClusterOrigin, CoedgeId, CoedgeRow, Contact, Continuity, CurveId, EdgeId,
    EdgeKind, EdgeRow, EdgeTag, FaceId, FaceRow, LoopId, LoopRow, Member, NONE, Origin, PCurveId,
    PlainOrigin, RegionId, ShellId, ShellKind, ShellRow, Side, SurfId, SurfaceRow, VertId, VertRow,
};
use crate::topo::check::{CheckError, Inv};

/// The most faces a [`SheetSnippet`] holds over its pieces. Provisional:
/// enough for the faces round the evidence's items, small enough to
/// draw at once.
pub const MAX_SNIPPET: usize = 16;

/// Faces of one leaf, cut out as a sheet (module docs).
#[derive(Clone, Debug)]
pub struct Piece {
    /// The leaf, by its place among the operation's operands.
    pub leaf: u32,
    /// The copy of the leaf's feature.
    pub instance: InstanceId,
    /// The invariant the piece's check leaves out: the one it is
    /// evidence of failing; `None` if it passes the full check.
    pub except: Option<Inv>,
    /// The sheet.
    pub sheet: Brep,
    /// Per face of the sheet, by its id, the leaf's face.
    pub faces: Vec<FaceId>,
    /// Per edge of the sheet, the leaf's edge.
    pub edges: Vec<EdgeId>,
    /// Per vertex of the sheet, the leaf's vertex.
    pub verts: Vec<VertId>,
}

/// The sheets a failure is about (module docs).
#[derive(Clone, Debug, Default)]
pub struct SheetSnippet {
    /// The pieces, in the order added.
    pub pieces: Vec<Piece>,
    /// What made its entities, where they aren't the leaves' own (a
    /// batch node's results, the derived arcs of a validity failure).
    pub label: Option<String>,
    /// How many faces were left out: past [`MAX_SNIPPET`], past the
    /// allowance, or of a piece that couldn't be cut.
    pub truncated: u32,
}

/// Two snippets are equal when they cut the same entities of the same
/// leaves, for the same invariant, with the same label: the sheets are
/// copies of those entities, so equal leaves give equal sheets.
impl PartialEq for SheetSnippet {
    fn eq(&self, other: &Self) -> bool {
        fn same(a: &Piece, b: &Piece) -> bool {
            (a.leaf, a.instance, a.except) == (b.leaf, b.instance, b.except)
                && (&a.faces, &a.edges, &a.verts) == (&b.faces, &b.edges, &b.verts)
        }
        self.label == other.label
            && self.truncated == other.truncated
            && self.pieces.len() == other.pieces.len()
            && (self.pieces.iter().zip(&other.pieces)).all(|(a, b)| same(a, b))
    }
}

/// Why faces can't be cut out of a leaf: a row the piece needs isn't
/// there, or a stored value the store refuses (a vertex not finite, an
/// edge not canonical).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CutError;

impl SheetSnippet {
    /// An empty snippet labelled `label` (module docs).
    pub fn labelled(label: impl Into<String>) -> SheetSnippet {
        SheetSnippet {
            label: Some(label.into()),
            ..SheetSnippet::default()
        }
    }

    /// The faces over its pieces.
    pub fn face_count(&self) -> usize {
        self.pieces.iter().map(|p| p.faces.len()).sum()
    }

    /// Whether it holds no piece.
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Each piece's check on `budget`: with its invariant left out, or
    /// the full sheet check; the first piece failing, by its place, and
    /// its error.
    pub fn check(&self, budget: &Budget) -> Result<(), (usize, CheckError)> {
        for (i, p) in self.pieces.iter().enumerate() {
            let r = match p.except {
                Some(inv) => p.sheet.check_except(inv, budget),
                None => p.sheet.check(budget),
            };
            r.map_err(|e| (i, e))?;
        }
        Ok(())
    }

    /// Adds the piece of `leaf` holding `faces` (those naming a face of
    /// it, each once, in geometric order: by key, ids breaking ties),
    /// cut for invariant `except`, as far as [`MAX_SNIPPET`] and
    /// `allowance` (a unit per face, loop and coedge copied) allow; the
    /// faces left out are counted. Whether a piece was added.
    pub(crate) fn add(
        &mut self,
        leaf: &Leaf<'_>,
        faces: &[FaceId],
        except: Option<Inv>,
        allowance: &mut Allowance,
    ) -> bool {
        let b = leaf.body;
        let mut faces: Vec<FaceId> = faces
            .iter()
            .copied()
            .filter(|f| f.index() < b.face_count())
            .collect();
        faces.sort_unstable_by_key(|&f| (b.faces.key[f.index()], f));
        faces.dedup();
        if faces.is_empty() {
            return false;
        }
        let room = MAX_SNIPPET.saturating_sub(self.face_count());
        if faces.len() > room {
            self.left_out(faces.len() - room);
            faces.truncate(room);
        }
        if faces.is_empty() {
            return false;
        }
        let rows = Rows::of(b, &faces);
        if !allowance.afford(rows.count()) {
            self.left_out(faces.len());
            return false;
        }
        match cut(b, &rows) {
            Ok((sheet, edges, verts)) => {
                self.pieces.push(Piece {
                    leaf: leaf.index,
                    instance: leaf.instance,
                    except,
                    sheet,
                    faces: rows.faces,
                    edges,
                    verts,
                });
                true
            }
            Err(CutError) => {
                self.left_out(faces.len());
                false
            }
        }
    }

    fn left_out(&mut self, n: usize) {
        self.truncated = self
            .truncated
            .saturating_add(u32::try_from(n).unwrap_or(u32::MAX));
    }
}

/// The leaf's rows a piece copies, each kind in id order.
struct Rows {
    faces: Vec<FaceId>,
    loops: Vec<LoopId>,
    coedges: Vec<CoedgeId>,
}

impl Rows {
    /// The rows of the faces `faces` of `b` (faces in id order after):
    /// their loops and coedges, found by the columns naming the faces.
    fn of(b: &Brep, faces: &[FaceId]) -> Rows {
        let mut faces = faces.to_vec();
        faces.sort_unstable();
        let kept = |f: FaceId| faces.binary_search(&f).is_ok();
        let loops = (0..b.loop_count() as u32)
            .map(LoopId)
            .filter(|l| kept(b.loops.face[l.index()]))
            .collect();
        let coedges = (0..b.coedge_count() as u32)
            .map(CoedgeId)
            .filter(|c| kept(b.coedges.face[c.index()]))
            .collect();
        Rows {
            faces,
            loops,
            coedges,
        }
    }

    /// The units copying them takes: a unit per face, loop and coedge
    /// (each coedge standing for its edge, vertex and geometry too).
    fn count(&self) -> u64 {
        let n = self.faces.len() + self.loops.len() + self.coedges.len();
        u64::try_from(n).unwrap_or(u64::MAX)
    }
}

/// A map of a kind's old ids, kept in order, to their new ids.
#[derive(Default)]
struct Map(BTreeMap<u32, u32>);

impl Map {
    /// From the old ids `ids` (any order, repeats allowed).
    fn of(ids: impl IntoIterator<Item = u32>) -> Map {
        let mut m = BTreeMap::new();
        for id in ids {
            m.insert(id, 0);
        }
        for (new, v) in m.values_mut().enumerate() {
            *v = new as u32;
        }
        Map(m)
    }

    /// The new id of `old`; `NONE` if it isn't kept.
    fn get(&self, old: u32) -> u32 {
        self.0.get(&old).copied().unwrap_or(NONE)
    }

    /// The old ids, by new id.
    fn olds(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.keys().copied()
    }
}

/// The sheet of `rows` of `b` (module docs), and the leaf ids of its
/// edges and vertices.
fn cut(b: &Brep, rows: &Rows) -> Result<(Brep, Vec<EdgeId>, Vec<VertId>), CutError> {
    let need = |ok: bool| if ok { Ok(()) } else { Err(CutError) };
    let (cs, es, vs) = (&b.coedges, &b.edges, &b.verts);
    for &c in &rows.coedges {
        let i = c.index();
        need(cs.edge[i].index() < b.edge_count() && cs.pcurve[i].index() < b.pcurve_count())?;
    }
    let faces = Map::of(rows.faces.iter().map(|f| f.0));
    let loops = Map::of(rows.loops.iter().map(|l| l.0));
    let coedges = Map::of(rows.coedges.iter().map(|c| c.0));
    let edges = Map::of(rows.coedges.iter().map(|&c| cs.edge[c.index()].0));
    for e in edges.olds() {
        let e = e as usize;
        need(es.v_start[e].index() < b.vert_count() && es.v_end[e].index() < b.vert_count())?;
        need(es.curve[e].index() < b.curve_count())?;
    }
    let verts = Map::of(
        edges
            .olds()
            .flat_map(|e| [es.v_start[e as usize].0, es.v_end[e as usize].0]),
    );
    let curves = Map::of(edges.olds().map(|e| es.curve[e as usize].0));
    let pcurves = Map::of(rows.coedges.iter().map(|&c| cs.pcurve[c.index()].0));
    let origin_surfaces = edges.olds().flat_map(|e| {
        es.origin[e as usize]
            .surfaces()
            .map(|s| s.0)
            .collect::<Vec<_>>()
    });
    let surfaces = Map::of(
        rows.faces
            .iter()
            .map(|&f| b.faces.surface[f.index()].0)
            .chain(origin_surfaces),
    );
    need(surfaces.olds().all(|s| (s as usize) < b.surface_count()))?;

    let start = OpStart {
        max_coord: 0.0,
        rho_op: b.status().op().rho_op(),
    };
    let mut s = Brep::new(BodyKind::Sheet, Status::new(b.eps().eps(), &start));
    let err = |_| CutError;
    for old in surfaces.olds() {
        let i = old as usize;
        s.push_surface(SurfaceRow {
            surface: b.surfaces.surface[i].clone(),
            fit: b.surfaces.fit[i].clone(),
            exact_rows: b.surfaces.exact_rows[i].clone(),
        })
        .map_err(err)?;
    }
    for old in curves.olds() {
        s.push_curve(b.curves3[old as usize].clone()).map_err(err)?;
    }
    for old in pcurves.olds() {
        s.push_pcurve(b.pcurves[old as usize].clone())
            .map_err(err)?;
    }
    // A vertex's or an edge's coedge: its own if kept, else the first
    // kept one `kept` accepts (starting at the vertex, using the edge).
    let held = |old: CoedgeId, kept: &dyn Fn(CoedgeId) -> bool| -> CoedgeId {
        let new = coedges.get(old.0);
        if new != NONE || old.is_none() {
            return CoedgeId(new);
        }
        (rows.coedges.iter().copied())
            .find(|&c| kept(c))
            .map_or(CoedgeId::NONE, |c| CoedgeId(coedges.get(c.0)))
    };
    let start_of = |c: CoedgeId| {
        let e = cs.edge[c.index()].index();
        if cs.reversed[c.index()] {
            es.v_end[e]
        } else {
            es.v_start[e]
        }
    };
    for old in verts.olds() {
        let i = old as usize;
        s.push_vert(VertRow {
            pos: vs.pos[i],
            tol: vs.tol[i],
            kind: vs.kind[i],
            coedge: held(vs.coedge[i], &|c| start_of(c) == VertId(old)),
            tag: vs.tag[i],
            attrs: vs.attrs[i],
        })
        .map_err(err)?;
    }
    // Whether an edge keeps both its uses: else it opens.
    let uses = |e: u32| {
        rows.coedges
            .iter()
            .filter(|&&c| cs.edge[c.index()].0 == e)
            .count()
    };
    for old in edges.olds() {
        let i = old as usize;
        let open = es.kind[i] != EdgeKind::Singular && uses(old) < 2;
        let origin = map_origin(&es.origin[i], &surfaces).ok_or(CutError)?;
        s.push_edge(EdgeRow {
            curve: CurveId(curves.get(es.curve[i].0)),
            t_range: es.t_range[i],
            v_start: VertId(verts.get(es.v_start[i].0)),
            v_end: VertId(verts.get(es.v_end[i].0)),
            kind: if open { EdgeKind::Regular } else { es.kind[i] },
            cont: if open {
                Continuity::default()
            } else {
                es.cont[i]
            },
            origin,
            tol: es.tol[i],
            dev: es.dev[i],
            embedded_join: es.embedded_join[i] && !open,
            coedge: held(es.coedge[i], &|c| cs.edge[c.index()] == EdgeId(old)),
            tag: es.tag[i].filter(|&t| !(open && t == EdgeTag::Seam)),
            attrs: es.attrs[i],
        })
        .map_err(err)?;
    }
    for &f in &rows.faces {
        let i = f.index();
        s.push_face(FaceRow {
            surface: SurfId(surfaces.get(b.faces.surface[i].0)),
            flipped: b.faces.flipped[i],
            shells: [ShellId::NONE; 2],
            first_loop: LoopId(loops.get(b.faces.first_loop[i].0)),
            key: b.faces.key[i],
            attrs: b.faces.attrs[i],
        })
        .map_err(err)?;
    }
    for &l in &rows.loops {
        let i = l.index();
        s.push_loop(LoopRow {
            face: FaceId(faces.get(b.loops.face[i].0)),
            first_coedge: CoedgeId(coedges.get(b.loops.first_coedge[i].0)),
            next_loop: LoopId(loops.get(b.loops.next_loop[i].0)),
            is_outer: b.loops.is_outer[i],
        })
        .map_err(err)?;
    }
    for &c in &rows.coedges {
        let i = c.index();
        s.push_coedge(CoedgeRow {
            edge: EdgeId(edges.get(cs.edge[i].0)),
            reversed: cs.reversed[i],
            face: FaceId(faces.get(cs.face[i].0)),
            lp: LoopId(loops.get(cs.lp[i].0)),
            next: CoedgeId(coedges.get(cs.next[i].0)),
            prev: CoedgeId(coedges.get(cs.prev[i].0)),
            pair: CoedgeId(coedges.get(cs.pair[i].0)),
            pcurve: PCurveId(pcurves.get(cs.pcurve[i].0)),
            seam_bound: cs.seam_bound[i],
        })
        .map_err(err)?;
    }
    open_shells(&mut s).map_err(err)?;
    let maps = (&verts, &edges, &faces);
    s.contacts = (b.contacts.iter())
        .filter_map(|c| map_contact(c, maps))
        .collect();
    s.names = b.names.clone();
    s.prov = b.prov.clone();
    s.eps = *b.eps();
    let edges = edges.olds().map(EdgeId).collect();
    let verts = verts.olds().map(VertId).collect();
    Ok((s, edges, verts))
}

/// `origin` with its surfaces mapped by `surfaces`; `None` if one isn't
/// kept.
fn map_origin(origin: &Origin, surfaces: &Map) -> Option<Origin> {
    let cluster = |c: ClusterOrigin| {
        let sa = surfaces.get(c.sa.0);
        let sb = surfaces.get(c.sb.0);
        (sa != NONE && sb != NONE).then_some(ClusterOrigin {
            sa: SurfId(sa),
            sb: SurfId(sb),
            ..c
        })
    };
    let plain = |p: PlainOrigin| match p {
        PlainOrigin::Cluster(c) => cluster(c).map(PlainOrigin::Cluster),
        p => Some(p),
    };
    Some(match origin {
        Origin::Plain(p) => Origin::Plain(plain(*p)?),
        Origin::MovedJoin { prior, clusters } => Origin::MovedJoin {
            prior: plain(*prior)?,
            clusters: (clusters.iter().map(|&c| cluster(c))).collect::<Option<_>>()?,
        },
    })
}

/// Contact `c` with its members mapped; `None` unless all are kept.
fn map_contact(c: &Contact, (verts, edges, faces): (&Map, &Map, &Map)) -> Option<Contact> {
    let some = |id: u32| (id != NONE).then_some(id);
    let member = |m: Member| -> Option<Member> {
        Some(match m {
            Member::Vert(v) => Member::Vert(VertId(some(verts.get(v.0))?)),
            Member::Edge(e) => Member::Edge(EdgeId(some(edges.get(e.0))?)),
            Member::EdgeAt { edge, t } => Member::EdgeAt {
                edge: EdgeId(some(edges.get(edge.0))?),
                t,
            },
            Member::FaceAt { face, uv } => Member::FaceAt {
                face: FaceId(some(faces.get(face.0))?),
                uv,
            },
        })
    };
    let members = (c.members.iter())
        .map(|&[a, b]| Some([member(a)?, member(b)?]))
        .collect::<Option<Vec<_>>>()?;
    Some(Contact {
        kind: c.kind,
        members,
        cert: c.cert.clone(),
    })
}

/// Joins the faces' sides of sheet `s` into open shells: per part
/// connected through paired coedges (parts by their least face), one of
/// the `Out` sides and one of the `In` sides.
fn open_shells(s: &mut Brep) -> Result<(), crate::topo::brep::StoreError> {
    let n = s.face_count();
    let mut part: Vec<usize> = (0..n).collect();
    fn root(part: &mut [usize], mut f: usize) -> usize {
        while part[f] != f {
            part[f] = part[part[f]];
            f = part[f];
        }
        f
    }
    for c in 0..s.coedge_count() {
        let p = s.coedges.pair[c];
        if p.index() >= s.coedge_count() {
            continue;
        }
        let (f, g) = (s.coedges.face[c].index(), s.coedges.face[p.index()].index());
        if f < n && g < n {
            let (a, b) = (root(&mut part, f), root(&mut part, g));
            part[a.max(b)] = a.min(b);
        }
    }
    let mut parts: BTreeMap<usize, Vec<FaceId>> = BTreeMap::new();
    for f in 0..n {
        let r = root(&mut part, f);
        parts.entry(r).or_default().push(FaceId(f as u32));
    }
    for faces in parts.values() {
        for side in [Side::Out, Side::In] {
            let sh = s.push_shell(ShellRow {
                sides: faces.iter().map(|&f| (f, side)).collect(),
                region: RegionId::NONE,
                kind: ShellKind::Open,
            })?;
            for &f in faces {
                s.faces.shells[f.index()][side.index()] = sh;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
