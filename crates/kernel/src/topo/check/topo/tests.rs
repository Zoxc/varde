//! Invariants 1 to 3 on built bodies: planar polyhedra (a cube, an
//! L-prism whose bottom pinches at its reflex corner, two cubes touching
//! along an edge), planar sheets (a square, a frame, a bowtie) and a
//! half-sphere sheet with its poles' singular edges. Each passes; each
//! defect seeded into it is caught by the check that names it, the
//! invariants after a skipped one meeting a broken body without
//! panicking.

use std::collections::BTreeMap;

use varde_decide::bezier::Dir;
use varde_decide::num::storage::OpStart;

use super::*;
use crate::geom::curve::Curve3D;
use crate::geom::form::{PlaneKey, SphereKey};
use crate::geom::lattice::{CELL, Lattice};
use crate::geom::pcurve::Span2;
use crate::infra::budget::{Budget, Work};
use crate::infra::ops::{FeatureId, InstanceId, Status, Tolerance};
use crate::topo::brep::{ExactRows, FaceKey, FacePart, PoleEnd, VertTag};
use crate::topo::build::{Builder, Use};
use crate::topo::check::{CheckError, CheckFailure, Inv, Invariants, Scope, run};

fn status() -> Status {
    let start = OpStart {
        max_coord: 4.0,
        rho_op: 0.0,
    };
    Status::new(Tolerance::new(1e-3).expect("in range"), &start)
}

fn key(j: usize) -> FaceKey {
    FaceKey {
        feature: FeatureId(1),
        part: FacePart::Primitive(j as u32),
        instance: InstanceId::ORIGINAL,
    }
}

fn line(a: [f64; 3], b: [f64; 3]) -> Curve3D {
    let m = [0, 1, 2].map(|k| (a[k] + b[k]) * 0.5);
    let pt = |p: [f64; 3]| [p[0], p[1], p[2], 1.0];
    Curve3D::new(vec![[pt(a), pt(m), pt(b)]], vec![0.0, 1.0]).expect("a line")
}

fn uv_line(a: [f64; 2], b: [f64; 2]) -> PCurve {
    let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let span: Span2 = [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]];
    PCurve::new(vec![span], vec![0.0, 1.0]).expect("a UV line")
}

fn newell(pts: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for (k, a) in pts.iter().enumerate() {
        let b = pts[(k + 1) % pts.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// A face of a polyhedron: its loops as vertex indices, counter-clockwise
/// seen from outside, the outer first; and its component, whose edges
/// are its own.
struct PolyFace {
    loops: Vec<Vec<usize>>,
    part: usize,
}

/// Planar faces on vertex positions.
struct Poly {
    pos: Vec<[f64; 3]>,
    faces: Vec<PolyFace>,
}

fn faces(lists: &[&[usize]]) -> Vec<PolyFace> {
    lists
        .iter()
        .map(|l| PolyFace {
            loops: vec![l.to_vec()],
            part: 0,
        })
        .collect()
}

/// The polyhedron as a body of kind `kind`: a plane per face (each
/// vertex's UV its offset along the lattice's axes, the same bits for
/// every coedge at it), an edge per vertex pair and component, its curve
/// and p-curves lines; per component a shell of the faces' `Out` sides and
/// one of their `In` sides, on a solid the first an island of the
/// unbounded region and the second a lump's outer shell.
fn build(p: &Poly, kind: BodyKind) -> Brep {
    let mut b = Builder::new(kind, status());
    let verts: Vec<VertId> = p.pos.iter().map(|&x| b.add_vert(x).unwrap()).collect();
    let mut edges: BTreeMap<(usize, usize, usize), EdgeId> = BTreeMap::new();
    let mut parts: BTreeMap<usize, Vec<FaceId>> = BTreeMap::new();
    for (j, f) in p.faces.iter().enumerate() {
        let outer: Vec<[f64; 3]> = f.loops[0].iter().map(|&i| p.pos[i]).collect();
        let n = newell(&outer);
        let plane = PlaneKey::new(n, outer[0]).unwrap();
        let lattice = Lattice::new(plane.into()).unwrap();
        let surface = Surface::new(lattice.clone(), lattice.half(0).unwrap()).unwrap();
        let s = b.add_surface(surface, None, ExactRows::default()).unwrap();
        let flipped = (0..3).map(|k| n[k] * plane.normal()[k]).sum::<f64>() < 0.0;
        let face = b.add_face(s, flipped, key(j)).unwrap();
        let (o, [ua, va, _]) = (lattice.origin(), lattice.axes());
        let uv = |x: [f64; 3]| {
            let d = [x[0] - o[0], x[1] - o[1], x[2] - o[2]];
            [ua, va].map(|a| (d[0] * a[0] + d[1] * a[1] + d[2] * a[2]) / CELL + 0.5)
        };
        for (k, l) in f.loops.iter().enumerate() {
            let uses: Vec<Use> = (0..l.len())
                .map(|i| {
                    let (from, to) = (l[i], l[(i + 1) % l.len()]);
                    let (lo, hi) = (from.min(to), from.max(to));
                    let e = *edges.entry((lo, hi, f.part)).or_insert_with(|| {
                        let c = b.add_curve(line(p.pos[lo], p.pos[hi])).unwrap();
                        b.add_edge(c, [0.0, 1.0], [verts[lo], verts[hi]]).unwrap()
                    });
                    let pcurve = b.add_pcurve(uv_line(uv(p.pos[lo]), uv(p.pos[hi]))).unwrap();
                    Use {
                        edge: e,
                        reversed: from > to,
                        pcurve,
                        seam_bound: 0.0,
                    }
                })
                .collect();
            b.add_loop(face, k == 0, &uses).unwrap();
        }
        parts.entry(f.part).or_default().push(face);
    }
    let mut islands = Vec::new();
    let mut lumps = Vec::new();
    for faces in parts.values() {
        let sides = |side| faces.iter().map(|&f| (f, side)).collect::<Vec<_>>();
        let (out, inner) = match kind {
            BodyKind::Solid => (ShellKind::Island, ShellKind::Outer),
            BodyKind::Sheet => (ShellKind::Open, ShellKind::Open),
        };
        islands.push(b.add_shell(out, sides(Side::Out)).unwrap());
        lumps.push(b.add_shell(inner, sides(Side::In)).unwrap());
    }
    if kind == BodyKind::Solid {
        let none = RegionId::NONE;
        b.add_region(RegionKind::Empty, ShellId::NONE, islands, none)
            .unwrap();
        for s in lumps {
            b.add_region(RegionKind::Material, s, Vec::new(), none)
                .unwrap();
        }
    }
    b.finish().unwrap().0
}

/// The unit cube.
fn cube() -> Poly {
    let pos = (0..8)
        .map(|i| [(i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64])
        .collect();
    let faces = faces(&[
        &[0, 2, 3, 1],
        &[4, 5, 7, 6],
        &[0, 1, 5, 4],
        &[2, 6, 7, 3],
        &[0, 4, 6, 2],
        &[1, 3, 7, 5],
    ]);
    Poly { pos, faces }
}

/// The pinched vertex of [`pinched`]: the L's reflex corner.
const PINCH: usize = 3;

/// An L-prism `[0, 4]² ∖ [2, 4]²` of height 2 whose bottom is two
/// faces: the square `[1, 2]²` at the reflex corner, and the rest, whose
/// boundary passes the corner twice (the shape `fC ∪ lNeck` leaves). As
/// one loop the rest's two corners there lie on the corner's one fan
/// cycle; `split`, the loop is an outer loop and a hole, each with its own
/// corner there, and the corner has two fans.
fn pinched(split: bool) -> Poly {
    let outline = [
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 2.0],
        [2.0, 2.0],
        [2.0, 4.0],
        [0.0, 4.0],
    ];
    let mut pos: Vec<[f64; 3]> = outline.iter().map(|p| [p[0], p[1], 0.0]).collect();
    pos.extend(outline.iter().map(|p| [p[0], p[1], 2.0]));
    pos.extend([[2.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 2.0, 0.0]]);
    let mut lists: Vec<Vec<Vec<usize>>> = (0..6)
        .map(|i| vec![vec![i, (i + 1) % 6, 6 + (i + 1) % 6, 6 + i]])
        .collect();
    lists.push(vec![(6..12).collect()]);
    lists.push(vec![vec![14, 3, 12, 13]]);
    lists.push(if split {
        vec![vec![5, 4, 3, 2, 1, 0], vec![3, 14, 13, 12]]
    } else {
        vec![vec![5, 4, 3, 14, 13, 12, 3, 2, 1, 0]]
    });
    let faces = lists
        .into_iter()
        .map(|loops| PolyFace { loops, part: 0 })
        .collect();
    Poly { pos, faces }
}

/// The unit cube and the cube `[1, 2]² × [0, 1]` touching it along the
/// line `x = y = 1`, sharing the vertices there (3 and 7), each with its
/// own edge between them.
fn edge_touching() -> Poly {
    let mut p = cube();
    let mut index = [0usize; 8];
    for (i, at) in index.iter_mut().enumerate() {
        *at = match i {
            0 => 3,
            4 => 7,
            _ => {
                let at = [(i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64];
                p.pos.push([1.0 + at[0], 1.0 + at[1], at[2]]);
                p.pos.len() - 1
            }
        };
    }
    let second: Vec<PolyFace> = (p.faces.iter())
        .map(|f| PolyFace {
            loops: vec![f.loops[0].iter().map(|&i| index[i]).collect()],
            part: 1,
        })
        .collect();
    p.faces.extend(second);
    p
}

const SQUARE: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 0.0],
];

/// The unit square as a sheet of two triangles.
fn square() -> Poly {
    Poly {
        pos: SQUARE.to_vec(),
        faces: faces(&[&[0, 1, 2], &[0, 2, 3]]),
    }
}

/// A square sheet `[0, 3]²` with the square hole `[1, 2]²`.
fn frame() -> Poly {
    let mut pos: Vec<[f64; 3]> = SQUARE.iter().map(|p| p.map(|x| 3.0 * x)).collect();
    pos.extend(SQUARE.iter().map(|p| [1.0 + p[0], 1.0 + p[1], 0.0]));
    Poly {
        pos,
        faces: vec![PolyFace {
            loops: vec![vec![0, 1, 2, 3], vec![4, 7, 6, 5]],
            part: 0,
        }],
    }
}

/// Two triangles of a sheet sharing only vertex 2.
fn bowtie() -> Poly {
    let pos = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [2.0, 1.0, 0.0],
        [2.0, 2.0, 0.0],
    ];
    Poly {
        pos,
        faces: faces(&[&[0, 1, 2], &[2, 3, 4]]),
    }
}

/// The unit sphere's half `u ∈ [−1, 1]` (two angle cells), all
/// latitudes, as a sheet of one face: its loop the south pole's singular
/// edge, the meridian `u = 1` up, the north pole's singular edge and the
/// meridian `u = −1` down, every p-curve iso, the meridians on the
/// rectangle's sides.
fn half_sphere() -> Brep {
    let sphere = SphereKey::new([0.0; 3], 1.0, [0.0, 0.0, 1.0]).unwrap();
    let lattice = Lattice::new(sphere.into()).unwrap();
    let surface = Surface::new(lattice.clone(), lattice.half(-1).unwrap()).unwrap();
    let at = |u: f64, v: f64| surface.eval([u, v]).unwrap().map(|x| x.v);
    let (south, north) = (at(-1.0, -1.0), at(-1.0, 1.0));
    let mut b = Builder::new(BodyKind::Sheet, status());
    let s = b
        .add_surface(surface.clone(), None, ExactRows::default())
        .unwrap();
    let vs = b.add_pole(south, VertTag::Pole(PoleEnd::S)).unwrap();
    let vn = b.add_pole(north, VertTag::Pole(PoleEnd::N)).unwrap();
    let pole_edge = |b: &mut Builder, v: VertId, p: [f64; 3]| {
        let c = b.add_curve(line(p, p)).unwrap();
        let e = b.add_edge(c, [0.0, 1.0], [v, v]).unwrap();
        b.set_edge_kind(e, EdgeKind::Singular);
        e
    };
    let es = pole_edge(&mut b, vs, south);
    let en = pole_edge(&mut b, vn, north);
    let r = std::f64::consts::FRAC_1_SQRT_2;
    let quarter = |p: [f64; 3], q: [f64; 3]| {
        let corner = [0, 1, 2].map(|k| p[k] + q[k]);
        [
            [p[0], p[1], p[2], 1.0],
            [corner[0], corner[1], corner[2], r],
            [q[0], q[1], q[2], 1.0],
        ]
    };
    let meridian = |b: &mut Builder, u: f64| {
        let eq = at(u, 0.0);
        let c = Curve3D::new(
            vec![quarter(south, eq), quarter(eq, north)],
            vec![0.0, 1.0, 2.0],
        )
        .unwrap();
        let c = b.add_curve(c).unwrap();
        b.add_edge(c, [0.0, 2.0], [vs, vn]).unwrap()
    };
    let (east, west) = (meridian(&mut b, 1.0), meridian(&mut b, -1.0));
    let iso = |fixed: Dir, at: f64, knots: Vec<f64>| {
        let half = |a: f64, z: f64| PSpan::Iso {
            fixed,
            at,
            m: [[a, 1.0], [z, 1.0]],
        };
        PCurve::from_spans(vec![half(-1.0, 0.0), half(0.0, 1.0)], knots).unwrap()
    };
    let f = b.add_face(s, false, key(0)).unwrap();
    let use_ = |b: &mut Builder, edge, reversed, p| Use {
        edge,
        reversed,
        pcurve: b.add_pcurve(p).unwrap(),
        seam_bound: 0.0,
    };
    let uses = [
        use_(&mut b, es, false, iso(Dir::V, -1.0, vec![0.0, 0.5, 1.0])),
        use_(&mut b, east, false, iso(Dir::U, 1.0, vec![0.0, 1.0, 2.0])),
        use_(&mut b, en, true, iso(Dir::V, 1.0, vec![0.0, 0.5, 1.0])),
        use_(&mut b, west, true, iso(Dir::U, -1.0, vec![0.0, 1.0, 2.0])),
    ];
    b.add_loop(f, true, &uses).unwrap();
    b.add_shell(ShellKind::Open, vec![(f, Side::Out)]).unwrap();
    b.add_shell(ShellKind::Open, vec![(f, Side::In)]).unwrap();
    b.finish().unwrap().0
}

/// A hook passing everything.
struct Passing;

impl Invariant for Passing {
    fn kinds(&self) -> &[ItemKind] {
        &[]
    }

    fn check(&self, _: &Cx<'_>, _: Item, _: &mut Ledger<'_>) -> Checked {
        Ok(())
    }
}

/// Invariant 1's hook on some kinds of item only.
struct Only(&'static [ItemKind]);

impl Invariant for Only {
    fn kinds(&self) -> &[ItemKind] {
        self.0
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Checked {
        Topology.check(cx, item, ledger)
    }
}

/// Invariants 1 to 3, `first` in place of 1's hook, the rest passing.
fn table(first: &dyn Invariant) -> Invariants<'_> {
    [
        first, &Joins, &Domain, &Passing, &Passing, &Passing, &Passing, &Passing, &Passing,
        &Passing,
    ]
}

/// Invariants 1 to 3 on `b`, `except` left out.
fn check_with(b: &Brep, first: &dyn Invariant, except: Option<Inv>) -> Result<(), CheckError> {
    let work = &mut Work::new(&Budget::DEFAULT);
    run(b, Scope::Full, except, &table(first), work, 0)
}

fn check13(b: &Brep) -> Result<(), CheckError> {
    #[cfg(not(target_arch = "wasm32"))]
    return crate::infra::par::assert_deterministic(|| check_with(b, &Topology, None));
    #[cfg(target_arch = "wasm32")]
    return check_with(b, &Topology, None);
}

fn failure(r: Result<(), CheckError>) -> CheckFailure {
    match r {
        Err(CheckError::Failed(f)) => f,
        other => panic!("not a failure: {other:?}"),
    }
}

/// The failure of `b`'s check: its invariant and check; and the hooks of
/// 2 and 3 run on it with 1 left out, without panicking.
fn fails(b: &Brep) -> (Inv, u32) {
    let f = failure(check13(b));
    assert_eq!(f.finding, Finding::Defect);
    if f.inv == Inv::Topology {
        let _ = check_with(b, &Topology, Some(Inv::Topology));
    }
    (f.inv, f.check)
}

fn topo(c: TopoCheck) -> (Inv, u32) {
    (Inv::Topology, c as u32)
}

fn joins(c: JoinCheck) -> (Inv, u32) {
    (Inv::Joins, c as u32)
}

fn domain(c: DomainCheck) -> (Inv, u32) {
    (Inv::Domain, c as u32)
}

/// The coedge running from vertex `a` to `z` (input indices, which the
/// builder keeps as ids), on the face numbered `face` if given.
fn coedge_at(b: &Brep, a: usize, z: usize, face: Option<usize>) -> CoedgeId {
    (0..b.coedge_count() as u32)
        .map(CoedgeId)
        .find(|&c| {
            b.coedge_vert(c, false) == VertId(a as u32)
                && b.coedge_vert(c, true) == VertId(z as u32)
                && face.is_none_or(|f| b.face(c) == FaceId(f as u32))
        })
        .expect("a coedge")
}

/// `b` with every p-curve of face `f` moved by `du` in `u`.
fn shift_face(mut b: Brep, f: FaceId, du: f64) -> Brep {
    for c in 0..b.coedge_count() {
        if b.coedges.face[c] != f {
            continue;
        }
        let id = b.coedges.pcurve[c].index();
        let p = &b.pcurves[id];
        let spans = (p.q_spans().unwrap().into_iter())
            .map(|s| s.map(|q| [q[0] + du, q[1], q[2]]))
            .collect();
        b.pcurves[id] = PCurve::new(spans, p.knots().to_vec()).unwrap();
    }
    b
}

#[test]
fn valid_bodies_pass() {
    for (name, b) in [
        ("cube", build(&cube(), BodyKind::Solid)),
        ("pinched", build(&pinched(false), BodyKind::Solid)),
        ("square", build(&square(), BodyKind::Sheet)),
        ("frame", build(&frame(), BodyKind::Sheet)),
        ("cube sheet", build(&cube(), BodyKind::Sheet)),
        ("half sphere", half_sphere()),
    ] {
        assert_eq!(check13(&b), Ok(()), "{name}");
    }
    for kind in [BodyKind::Solid, BodyKind::Sheet] {
        assert_eq!(check13(&Brep::new(kind, status())), Ok(()));
    }
}

/// A loop visiting a vertex twice at a pinch passes: its two corners lie
/// on the vertex's one fan cycle. Split into an outer loop and a hole
/// through the vertex, each loop's corner makes its own fan.
#[test]
fn a_pinch_is_one_loop_with_one_fan() {
    let one = build(&pinched(false), BodyKind::Solid);
    let v = VertId(PINCH as u32);
    let corners = |b: &Brep| {
        (0..b.coedge_count() as u32)
            .map(CoedgeId)
            .filter(|&c| b.coedge_vert(c, true) == v)
            .collect::<Vec<_>>()
    };
    let bottom = FaceId(8);
    let on_bottom: Vec<_> = (corners(&one).into_iter())
        .filter(|&c| one.face(c) == bottom)
        .collect();
    assert_eq!(on_bottom.len(), 2);
    assert_eq!(one.coedge_loop(on_bottom[0]), one.coedge_loop(on_bottom[1]));
    assert_eq!(check13(&one), Ok(()));
    let split = build(&pinched(true), BodyKind::Solid);
    let f = failure(check13(&split));
    assert_eq!((f.inv, f.check), topo(TopoCheck::Fan));
    assert_eq!(f.item, Item::Vert(v));
}

/// Two cubes touching along an edge, as one edge with four coedges: the
/// edge has more than a coedge and its pair, whichever way the pairs
/// run. With the pairs crossed every fan is one cycle, so only the edge's
/// coedges tell; with each cube's pair kept, the shared vertices have two
/// fans, one per cube.
#[test]
fn an_edge_with_four_coedges_is_caught() {
    let touching = build(&edge_touching(), BodyKind::Solid);
    // Sharing the vertices with an edge each: two fans at each.
    let f = failure(check13(&touching));
    assert_eq!((f.inv, f.check), topo(TopoCheck::Fan));
    let edges: Vec<EdgeId> = (0..touching.edge_count() as u32)
        .map(EdgeId)
        .filter(|&e| touching.ends(e) == [VertId(3), VertId(7)])
        .collect();
    let [e1, e2] = edges[..] else {
        panic!("two edges on the line")
    };
    for crossed in [false, true] {
        let mut b = touching.clone();
        let on = |b: &Brep, e: EdgeId| {
            let mut cs: Vec<CoedgeId> = (0..b.coedge_count() as u32)
                .map(CoedgeId)
                .filter(|&c| b.edge(c) == e)
                .collect();
            cs.sort_by_key(|&c| b.reversed(c));
            cs
        };
        let (first, second) = (on(&b, e1), on(&b, e2));
        for &c in &second {
            b.coedges.edge[c.index()] = e1;
        }
        if crossed {
            // Each first-cube coedge paired with the second cube's one
            // running the other way.
            for (a, z) in [(first[0], second[1]), (first[1], second[0])] {
                b.coedges.pair[a.index()] = z;
                b.coedges.pair[z.index()] = a;
            }
        }
        let mut edit = Builder::edit(b);
        edit.remove_edge(e2).unwrap();
        let b = edit.finish().unwrap().0;
        let f = failure(check13(&b));
        assert_eq!((f.inv, f.check), topo(TopoCheck::EdgeUses), "{crossed}");
        let fans = check_with(&b, &Only(&[ItemKind::Vert]), None);
        if crossed {
            assert_eq!(fans, Ok(()));
        } else {
            assert_eq!(failure(fans).check, TopoCheck::Fan as u32);
        }
    }
}

/// A sheet's fans run along its open boundary: a vertex where two faces
/// meet only there has two.
#[test]
fn a_sheet_vertex_has_one_fan() {
    let b = build(&bowtie(), BodyKind::Sheet);
    let f = failure(check13(&b));
    assert_eq!(
        (f.inv, f.check, f.item),
        (Inv::Topology, TopoCheck::Fan as u32, Item::Vert(VertId(2)))
    );
}

/// A defect seeded into a body.
type Mutation = Box<dyn Fn(&mut Brep)>;

/// Each topology defect seeded into a valid body, caught by its check.
#[test]
fn topology_defects_are_caught() {
    use TopoCheck as T;
    let solid = build(&cube(), BodyKind::Solid);
    let c = coedge_at(&solid, 0, 2, None);
    let e = solid.edge(c);
    let other = coedge_at(&solid, 5, 4, None);
    let cases: Vec<(&str, Mutation, TopoCheck)> = vec![
        (
            "a curve id naming nothing",
            Box::new(|b| b.edges.curve[0] = crate::topo::brep::CurveId(99)),
            T::EdgeIds,
        ),
        (
            "an edge holding another edge's coedge",
            Box::new(move |b| b.edges.coedge[e.index()] = other),
            T::EdgeCoedge,
        ),
        (
            "a p-curve id naming nothing",
            Box::new(move |b| b.coedges.pcurve[c.index()].0 = 999),
            T::CoedgeIds,
        ),
        (
            "`prev` not inverse to `next`",
            Box::new(move |b| b.coedges.prev[c.index()] = other),
            T::NextPrev,
        ),
        (
            "a loop on another face",
            Box::new(move |b| {
                let l = b.coedges.lp[c.index()];
                b.loops.face[l.index()] = b.coedges.face[other.index()];
            }),
            T::InLoop,
        ),
        (
            "a coedge turned round",
            Box::new(move |b| b.coedges.reversed[c.index()] ^= true),
            T::Corner,
        ),
        (
            "a pair not paired back",
            Box::new(move |b| b.coedges.pair[c.index()] = other),
            T::Pair,
        ),
        (
            "an edge unpaired on a solid",
            Box::new(move |b| {
                let p = b.coedges.pair[c.index()];
                b.coedges.pair[c.index()] = CoedgeId::NONE;
                b.coedges.pair[p.index()] = CoedgeId::NONE;
            }),
            T::EdgeUses,
        ),
        (
            "a paired edge marked singular",
            Box::new(move |b| b.edges.kind[e.index()] = EdgeKind::Singular),
            T::EdgeUses,
        ),
        (
            "a vertex holding a coedge arriving at it",
            Box::new(move |b| b.verts.coedge[2] = c),
            T::VertCoedge,
        ),
        (
            "a face with no loop",
            Box::new(|b| b.faces.first_loop[1] = LoopId::NONE),
            T::FaceLoops,
        ),
        (
            "a surface id naming nothing",
            Box::new(|b| b.faces.surface[1] = crate::topo::brep::SurfId(99)),
            T::FaceIds,
        ),
        (
            "a solid face's side in no shell",
            Box::new(|b| b.faces.shells[1][1] = ShellId::NONE),
            T::FaceShells,
        ),
        (
            "an open shell on a solid",
            Box::new(|b| b.shells.kind[0] = ShellKind::Open),
            T::ShellKind,
        ),
        (
            "a shell missing a side naming it",
            Box::new(|b| {
                b.shells.sides[0].pop();
            }),
            T::ShellSides,
        ),
        (
            "a shell listing a side twice",
            Box::new(|b| {
                let s = b.shells.sides[0][0];
                b.shells.sides[0][1] = s;
            }),
            T::ShellSides,
        ),
        (
            "a solid's shell in no region",
            Box::new(|b| b.shells.region[1] = RegionId::NONE),
            T::ShellRegion,
        ),
        (
            "the unbounded region a lump",
            Box::new(|b| b.regions.kind[0] = RegionKind::Material),
            T::RegionKind,
        ),
        (
            "a region missing its island",
            Box::new(|b| b.regions.islands[0].clear()),
            T::RegionShells,
        ),
        (
            "an island kept as an outer shell",
            Box::new(|b| b.shells.kind[0] = ShellKind::Outer),
            T::RegionShells,
        ),
        (
            "a region its own container",
            Box::new(|b| b.regions.container[1] = RegionId(1)),
            T::Container,
        ),
    ];
    for (name, mutate, check) in cases {
        let mut b = solid.clone();
        mutate(&mut b);
        assert_eq!(fails(&b), topo(check), "{name}");
    }
    // A sheet's shapes on a solid.
    let mut b = build(&square(), BodyKind::Sheet);
    b.kind = BodyKind::Solid;
    assert_eq!(fails(&b), topo(T::Open));
    let mut b = build(&cube(), BodyKind::Solid);
    b.kind = BodyKind::Sheet;
    assert_eq!(fails(&b), topo(T::ShellKind));
    // A frame's hole whose coedges name the outer loop: two cycles in one
    // loop.
    let mut b = build(&frame(), BodyKind::Sheet);
    let hole = LoopId(1);
    for c in 0..b.coedge_count() {
        if b.coedges.lp[c] == hole {
            b.coedges.lp[c] = LoopId(0);
        }
    }
    assert_eq!(fails(&b), topo(T::LoopCycle));
    // A frame whose loop list drops the hole.
    let mut b = build(&frame(), BodyKind::Sheet);
    b.loops.next_loop[0] = LoopId::NONE;
    assert_eq!(fails(&b), topo(T::FaceLoops));
    // A loop starting at another loop's coedge.
    let mut b = build(&frame(), BodyKind::Sheet);
    b.loops.first_coedge[1] = b.loops.first_coedge[0];
    assert_eq!(fails(&b), topo(T::LoopIds));
}

/// A singular coedge needs its pole: one pole vertex at both ends, the
/// curve that point, the p-curve iso on a side the tiles declare
/// singular.
#[test]
fn singular_defects_are_caught() {
    let sphere = half_sphere();
    let singular: Vec<CoedgeId> = (0..sphere.coedge_count() as u32)
        .map(CoedgeId)
        .filter(|&c| sphere.edge_kind(sphere.edge(c)) == EdgeKind::Singular)
        .collect();
    assert_eq!(singular.len(), 2);
    let c = singular[0];
    let e = sphere.edge(c);
    let v = sphere.ends(e)[0];
    let mut b = sphere.clone();
    b.verts.kind[v.index()] = VertKind::Regular;
    assert_eq!(fails(&b), topo(TopoCheck::Singular));
    // Its curve not the point.
    let mut b = sphere.clone();
    let p = b.pos(v);
    let q = [p[0] + 0.5, p[1], p[2]];
    let id = b.edges.curve[e.index()];
    b.curves3[id.index()] = line(p, q);
    assert_eq!(fails(&b), topo(TopoCheck::Singular));
    // Its p-curve on the equator, no singular side.
    let mut b = sphere.clone();
    let id = b.coedges.pcurve[c.index()];
    let old = &b.pcurves[id.index()];
    let moved: Vec<PSpan> = (old.spans().iter())
        .map(|s| match *s {
            PSpan::Iso { fixed, m, .. } => PSpan::Iso { fixed, at: 0.0, m },
            other => other,
        })
        .collect();
    b.pcurves[id.index()] = PCurve::from_spans(moved, old.knots().to_vec()).unwrap();
    assert_eq!(fails(&b), topo(TopoCheck::Singular));
    // A meridian marked singular: its coedge is unpaired, its ends two.
    let mut b = sphere.clone();
    let meridian = (0..b.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| b.edge_kind(e) == EdgeKind::Regular)
        .unwrap();
    b.edges.kind[meridian.index()] = EdgeKind::Singular;
    assert_eq!(fails(&b), topo(TopoCheck::Singular));
}

/// Each join defect, bit for bit, caught by its check.
#[test]
fn join_defects_are_caught() {
    use JoinCheck as J;
    let solid = build(&cube(), BodyKind::Solid);
    let c = coedge_at(&solid, 0, 2, None);
    let e = solid.edge(c);
    // Its ends swapped, its coedges turned with it: the topology holds.
    let mut b = solid.clone();
    let [vs, ve] = b.ends(e);
    b.edges.v_start[e.index()] = ve;
    b.edges.v_end[e.index()] = vs;
    for k in 0..b.coedge_count() {
        if b.coedges.edge[k] == e {
            b.coedges.reversed[k] ^= true;
        }
    }
    assert_eq!(fails(&b), joins(J::Canonical));
    let mut b = solid.clone();
    b.edges.t_range[e.index()] = [0.0, 0.5];
    assert_eq!(fails(&b), joins(J::Range));
    let mut b = solid.clone();
    b.edges.t_range[e.index()] = [1.0, 0.0];
    assert_eq!(fails(&b), joins(J::Range));
    // A vertex an ulp off its edges' curves.
    let mut b = solid.clone();
    b.verts.pos[7][0] = b.verts.pos[7][0].next_up();
    assert_eq!(fails(&b), joins(J::Ends));
    // A p-curve's end an ulp off the next one's start.
    let mut b = solid.clone();
    let id = b.coedges.pcurve[c.index()];
    let mut spans = b.pcurves[id.index()].q_spans().unwrap();
    spans[0][2][1] = spans[0][2][1].next_up();
    b.pcurves[id.index()] = PCurve::new(spans, vec![0.0, 1.0]).unwrap();
    assert_eq!(fails(&b), joins(J::Join));
    // A face's p-curves moved to straddle the tile line `u = 1`.
    let f = solid.face(c);
    let us: Vec<f64> = (0..solid.coedge_count())
        .filter(|&k| solid.coedges.face[k] == f)
        .flat_map(|k| {
            let p = &solid.pcurves[solid.coedges.pcurve[k].index()];
            [p.knot_point(0).unwrap()[0], p.knot_point(1).unwrap()[0]]
        })
        .collect();
    let mut sorted = us.clone();
    sorted.sort_by(f64::total_cmp);
    let (lo, hi) = (sorted[0], sorted[sorted.len() - 1]);
    assert!(lo < hi);
    let b = shift_face(solid.clone(), f, 1.0 - (lo + hi) * 0.5);
    assert_eq!(fails(&b), joins(J::Tiles));
    // A p-curve over another parameter range than its edge's.
    let mut b = solid.clone();
    let spans = b.pcurves[id.index()].q_spans().unwrap();
    b.pcurves[id.index()] = PCurve::new(spans, vec![0.0, 2.0]).unwrap();
    assert_eq!(fails(&b), joins(J::SameT));
}

/// A loop outside its surface's rectangle, and a span along its side
/// that isn't iso, are caught.
#[test]
fn domain_defects_are_caught() {
    let solid = build(&cube(), BodyKind::Solid);
    let b = shift_face(solid, FaceId(2), 1.0);
    assert_eq!(fails(&b), domain(DomainCheck::Inside));
    // The meridian `u = 1` as a Q span along the side.
    let mut b = half_sphere();
    let c = (0..b.coedge_count() as u32)
        .map(CoedgeId)
        .find(|&c| {
            let p = b.coedge_pcurve(c);
            matches!(p.spans()[0], PSpan::Iso { fixed: Dir::U, at, .. } if at == 1.0)
        })
        .unwrap();
    let id = b.coedges.pcurve[c.index()];
    let q = |v0: f64, v1: f64| -> Span2 {
        [[1.0, v0, 1.0], [1.0, (v0 + v1) * 0.5, 1.0], [1.0, v1, 1.0]]
    };
    b.pcurves[id.index()] =
        PCurve::new(vec![q(-1.0, 0.0), q(0.0, 1.0)], vec![0.0, 1.0, 2.0]).unwrap();
    assert_eq!(fails(&b), domain(DomainCheck::OnSide));
}
