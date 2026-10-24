//! The check's driver on hand-built bodies, with hooks for the
//! invariants that fail at chosen items: the first failure, its
//! invariant, kind of item and item, the same whatever the body's ids and
//! the thread count; the scope of a scoped check; the sheet variant; one
//! invariant left out; unbuilt parts refused; what a failure means by
//! role; and running out naming the invariant.

use varde_decide::num::storage::OpStart;
use varde_testkit::rng::Rng;

use super::*;
use crate::geom::curve::Curve3D;
use crate::geom::form::PlaneKey;
use crate::geom::lattice::Lattice;
use crate::geom::pcurve::{PCurve, Span2};
use crate::geom::surface::Surface;
use crate::infra::error::Cause;
use crate::infra::ops::{FeatureId, InstanceId, Status, Tolerance};
use crate::topo::brep::{
    AttrSetId, CoedgeRow, Continuity, EdgeKind, EdgeRow, ExactRows, FacePart, FaceRow, LoopRow,
    Origin, RegionRow, ShellRow, Side, SurfaceRow, VertKind, VertRow,
};

/// A polyhedron: vertex positions, and faces as loops of vertex indices
/// running counter-clockwise seen from outside.
struct Poly {
    pos: Vec<[f64; 3]>,
    faces: Vec<Vec<usize>>,
}

/// The unit cube.
fn cube() -> Poly {
    let pos = (0..8)
        .map(|i| [(i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64])
        .collect();
    let faces = vec![
        vec![0, 2, 3, 1],
        vec![4, 5, 7, 6],
        vec![0, 1, 5, 4],
        vec![2, 6, 7, 3],
        vec![0, 4, 6, 2],
        vec![1, 3, 7, 5],
    ];
    Poly { pos, faces }
}

/// A square pyramid, apex up: opposite walls share only the apex.
fn pyramid() -> Poly {
    let pos = vec![
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 2.0, 0.0],
        [0.0, 2.0, 0.0],
        [1.0, 1.0, 1.0],
    ];
    let faces = vec![
        vec![0, 3, 2, 1],
        vec![0, 1, 4],
        vec![1, 2, 4],
        vec![2, 3, 4],
        vec![3, 0, 4],
    ];
    Poly { pos, faces }
}

/// Where a polyhedron's inputs land among the body's ids: the vertex,
/// edge and face ids of the input's vertices, edges (in the order the
/// faces first reach them) and faces, and per face the corner its loop
/// starts at.
struct Perm {
    verts: Vec<usize>,
    edges: Vec<usize>,
    faces: Vec<usize>,
    starts: Vec<usize>,
}

/// The polyhedron's undirected edges, each its lower input index first,
/// in the order the faces first reach them.
fn input_edges(p: &Poly) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for f in &p.faces {
        for k in 0..f.len() {
            let (a, b) = (f[k], f[(k + 1) % f.len()]);
            let e = (a.min(b), a.max(b));
            if !out.contains(&e) {
                out.push(e);
            }
        }
    }
    out
}

fn shuffle(n: usize, rng: &mut Rng) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
    v
}

impl Perm {
    fn identity(p: &Poly) -> Perm {
        Perm {
            verts: (0..p.pos.len()).collect(),
            edges: (0..input_edges(p).len()).collect(),
            faces: (0..p.faces.len()).collect(),
            starts: vec![0; p.faces.len()],
        }
    }

    fn random(p: &Poly, rng: &mut Rng) -> Perm {
        Perm {
            verts: shuffle(p.pos.len(), rng),
            edges: shuffle(input_edges(p).len(), rng),
            faces: shuffle(p.faces.len(), rng),
            starts: (p.faces.iter())
                .map(|f| (rng.next_u64() % f.len() as u64) as usize)
                .collect(),
        }
    }
}

/// The position of each id among `ids`: the inverse permutation.
fn inverse(ids: &[usize]) -> Vec<usize> {
    let mut inv = vec![0; ids.len()];
    for (i, &id) in ids.iter().enumerate() {
        inv[id] = i;
    }
    inv
}

fn status() -> Status {
    let start = OpStart {
        max_coord: 2.0,
        rho_op: 0.0,
    };
    Status::new(Tolerance::new(1e-3).expect("in range"), &start)
}

/// Face `j` of the input's name.
fn face_key(j: usize) -> FaceKey {
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

fn uv_line(a: [f64; 3], b: [f64; 3]) -> PCurve {
    let span: Span2 = [
        [a[0], a[1], 1.0],
        [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, 1.0],
        [b[0], b[1], 1.0],
    ];
    PCurve::new(vec![span], vec![0.0, 1.0]).expect("a UV line")
}

fn plane() -> Surface {
    let lat = Lattice::new(PlaneKey::new([0.0, 0.0, 1.0], [0.0; 3]).unwrap().into()).unwrap();
    Surface::new(lat.clone(), lat.half(0).unwrap()).unwrap()
}

/// The polyhedron as a body of kind `kind`, its ids placed by `perm`:
/// every link set, one shell of the faces' outer sides and one lump. The
/// driver's tests read its topology only; every face is on one plane.
fn build(p: &Poly, perm: &Perm, kind: BodyKind) -> Brep {
    let mut b = Brep::new(kind, status());
    let surface = b
        .push_surface(SurfaceRow {
            surface: plane(),
            fit: None,
            exact_rows: ExactRows::default(),
        })
        .unwrap();
    let vid = |i: usize| VertId(perm.verts[i] as u32);
    for i in inverse(&perm.verts) {
        b.push_vert(VertRow {
            pos: p.pos[i],
            tol: 0.0,
            kind: VertKind::Regular,
            coedge: CoedgeId::NONE,
            tag: None,
            attrs: AttrSetId::NONE,
        })
        .unwrap();
    }
    let edges = input_edges(p);
    let eid = |a: usize, b: usize| {
        let k = edges
            .iter()
            .position(|&e| e == (a.min(b), a.max(b)))
            .unwrap();
        EdgeId(perm.edges[k] as u32)
    };
    for k in inverse(&perm.edges) {
        let (a, c) = edges[k];
        let (lo, hi) = if vid(a) < vid(c) { (a, c) } else { (c, a) };
        let curve = b.push_curve(line(p.pos[lo], p.pos[hi])).unwrap();
        b.push_edge(EdgeRow {
            curve,
            t_range: [0.0, 1.0],
            v_start: vid(lo),
            v_end: vid(hi),
            kind: EdgeKind::Regular,
            cont: Continuity::default(),
            origin: Origin::default(),
            tol: 0.0,
            dev: 0.0,
            embedded_join: false,
            coedge: CoedgeId::NONE,
            tag: None,
            attrs: AttrSetId::NONE,
        })
        .unwrap();
    }
    // Coedge ids face by face in id order, each loop from its start.
    let mut cid = std::collections::BTreeMap::new();
    let mut corners = Vec::new();
    for j in inverse(&perm.faces) {
        let f = &p.faces[j];
        let first = corners.len();
        for t in 0..f.len() {
            let k = (perm.starts[j] + t) % f.len();
            let (a, c) = (f[k], f[(k + 1) % f.len()]);
            cid.insert((a, c), CoedgeId(corners.len() as u32));
            let next = first + (t + 1) % f.len();
            let prev = first + (t + f.len() - 1) % f.len();
            corners.push((j, a, c, next, prev));
        }
    }
    for &(j, a, c, next, prev) in &corners {
        let e = eid(a, c);
        let reversed = vid(a) > vid(c);
        let (s, t) = if reversed { (c, a) } else { (a, c) };
        let pcurve = b.push_pcurve(uv_line(p.pos[s], p.pos[t])).unwrap();
        let pair = cid.get(&(c, a)).copied().unwrap_or(CoedgeId::NONE);
        let id = b
            .push_coedge(CoedgeRow {
                edge: e,
                reversed,
                face: FaceId(perm.faces[j] as u32),
                lp: LoopId(perm.faces[j] as u32),
                next: CoedgeId(next as u32),
                prev: CoedgeId(prev as u32),
                pair,
                pcurve,
                seam_bound: 0.0,
            })
            .unwrap();
        b.verts.coedge[vid(a).index()] = id;
        b.edges.coedge[e.index()] = id;
    }
    let first_coedge = |j: usize| {
        cid[&(
            p.faces[j][perm.starts[j]],
            p.faces[j][(perm.starts[j] + 1) % p.faces[j].len()],
        )]
    };
    for j in inverse(&perm.faces) {
        b.push_loop(LoopRow {
            face: FaceId(perm.faces[j] as u32),
            first_coedge: first_coedge(j),
            next_loop: LoopId::NONE,
            is_outer: true,
        })
        .unwrap();
    }
    for j in inverse(&perm.faces) {
        b.push_face(FaceRow {
            surface,
            flipped: false,
            shells: [ShellId(0); 2],
            first_loop: LoopId(perm.faces[j] as u32),
            key: face_key(j),
            attrs: AttrSetId::NONE,
        })
        .unwrap();
    }
    let sides = (0..p.faces.len())
        .map(|j| (FaceId(perm.faces[j] as u32), Side::Out))
        .collect();
    b.push_shell(ShellRow {
        sides,
        region: RegionId(0),
        kind: ShellKind::Outer,
    })
    .unwrap();
    b.push_region(RegionRow {
        kind: RegionKind::Material,
        outer: ShellId(0),
        islands: Vec::new(),
        container: RegionId::NONE,
    })
    .unwrap();
    b
}

/// What fails where: a fault per failing item.
type Fails = Box<dyn Fn(&Brep, Item) -> Option<Fault> + Sync>;

/// A test's hook: checks items of `kinds`, charging `units` each, and
/// fails where `fails` says.
struct Hook {
    kinds: Vec<ItemKind>,
    units: u64,
    fails: Fails,
}

impl Invariant for Hook {
    fn kinds(&self) -> &[ItemKind] {
        &self.kinds
    }

    fn check(&self, cx: &Cx<'_>, item: Item, ledger: &mut Ledger<'_>) -> Result<(), Stop<Fault>> {
        ledger.charge(Step::Work, self.units)?;
        match (self.fails)(cx.body(), item) {
            Some(fault) => Err(Stop::Failed(fault)),
            None => Ok(()),
        }
    }
}

const ALL_KINDS: [ItemKind; 9] = [
    ItemKind::Body,
    ItemKind::Vert,
    ItemKind::Edge,
    ItemKind::Coedge,
    ItemKind::Loop,
    ItemKind::Face,
    ItemKind::Shell,
    ItemKind::Region,
    ItemKind::Contact,
];

fn passing() -> Hook {
    Hook {
        kinds: ALL_KINDS.to_vec(),
        units: 1,
        fails: Box::new(|_, _| None),
    }
}

fn failing(
    kinds: &[ItemKind],
    fails: impl Fn(&Brep, Item) -> Option<Fault> + Sync + 'static,
) -> Hook {
    Hook {
        kinds: kinds.to_vec(),
        units: 1,
        fails: Box::new(fails),
    }
}

/// Every hook passing but those given, by invariant.
fn hooks(given: Vec<(Inv, Hook)>) -> [Hook; 10] {
    let mut hooks: [Hook; 10] = std::array::from_fn(|_| passing());
    for (inv, hook) in given {
        hooks[usize::from(inv.number() - 1)] = hook;
    }
    hooks
}

fn run_on(
    body: &Brep,
    scope: Scope<'_>,
    except: Option<Inv>,
    hooks: &[Hook; 10],
    budget: Budget,
) -> Result<(), CheckError> {
    let table: Invariants<'_> = std::array::from_fn(|i| &hooks[i] as &dyn Invariant);
    run(body, scope, except, &table, &mut Work::new(&budget), 0)
}

fn full(body: &Brep, hooks: &[Hook; 10]) -> Result<(), CheckError> {
    run_on(body, Scope::Full, None, hooks, Budget::DEFAULT)
}

fn fault(check: u32) -> Option<Fault> {
    Some(Fault {
        check,
        finding: Finding::Defect,
    })
}

fn failure(r: Result<(), CheckError>) -> CheckFailure {
    match r {
        Err(CheckError::Failed(f)) => f,
        other => panic!("not a failure: {other:?}"),
    }
}

/// `f`'s result, the same on any number of threads (natively; the web
/// runs every map in order).
fn deterministic<R: Send + std::fmt::Debug>(f: impl Fn() -> R + Sync) -> R {
    #[cfg(not(target_arch = "wasm32"))]
    return crate::infra::par::assert_deterministic(f);
    #[cfg(target_arch = "wasm32")]
    return f();
}

/// The item as the input knows it, whatever its id: a vertex's
/// position, an edge's end positions, a coedge's start and end and its
/// face's key, a loop's or face's key.
fn describe(b: &Brep, item: Item) -> String {
    match item {
        Item::Vert(v) => format!("vertex {:?}", b.pos(v)),
        Item::Edge(e) => {
            let mut ends = b.ends(e).map(|v| b.pos(v));
            ends.sort_by(|a, c| a.partial_cmp(c).unwrap());
            format!("edge {ends:?}")
        }
        Item::Coedge(c) => {
            let [s, t] = [false, true].map(|end| b.pos(b.coedge_vert(c, end)));
            format!("coedge {s:?} → {t:?} on {:?}", b.key(b.face(c)))
        }
        Item::Loop(l) => format!("loop of {:?}", b.key(b.loop_face(l))),
        Item::Face(f) => format!("face {:?}", b.key(f)),
        other => format!("{other:?}"),
    }
}

fn top(b: &Brep, v: VertId) -> bool {
    b.pos(v)[2] == 1.0
}

/// Hooks for invariant 3 failing on the cube's top edges, then its top
/// vertices, and for invariant 5 on the coedges of the top edges and the
/// faces at the origin; `only` picks one.
fn cube_hooks(only: usize) -> [Hook; 10] {
    let top_edge = |b: &Brep, e: EdgeId| b.ends(e).iter().all(|&v| top(b, v));
    let scenarios: [Box<dyn Fn() -> (Inv, Hook)>; 4] = [
        Box::new(move || {
            let hook = failing(
                &[ItemKind::Edge, ItemKind::Vert],
                move |b, item| match item {
                    Item::Edge(e) if top_edge(b, e) => fault(2),
                    Item::Vert(v) if top(b, v) => fault(1),
                    _ => None,
                },
            );
            (Inv::Domain, hook)
        }),
        Box::new(move || {
            let hook = failing(&[ItemKind::Coedge], move |b, item| match item {
                Item::Coedge(c) if top_edge(b, b.edge(c)) => fault(0),
                _ => None,
            });
            (Inv::Seams, hook)
        }),
        Box::new(|| {
            let hook = failing(&[ItemKind::Face], |b, item| match item {
                Item::Face(f) => {
                    let lp = b.first_loop(f);
                    b.loop_coedges(lp)
                        .any(|c| b.pos(b.coedge_vert(c, false)) == [0.0; 3])
                        .then_some(Fault {
                            check: 3,
                            finding: Finding::Crossing,
                        })
                }
                _ => None,
            });
            (Inv::Embedding, hook)
        }),
        Box::new(|| {
            let hook = failing(&[ItemKind::Loop], |b, item| match item {
                Item::Loop(l) => (b
                    .loop_coedges(l)
                    .filter(|&c| top(b, b.coedge_vert(c, false)))
                    .count()
                    == 2)
                    .then_some(Fault {
                        check: 0,
                        finding: Finding::Defect,
                    }),
                _ => None,
            });
            (Inv::Names, hook)
        }),
    ];
    hooks(vec![scenarios[only]()])
}

/// Of several failing items, the one reported is the first in the
/// geometric order: the same entity of the input, by position and name,
/// whatever ids the body gives its vertices, edges, faces and loop starts,
/// and the same on any number of threads.
#[test]
fn the_first_error_is_stable_under_permutation() {
    let p = cube();
    let expected = [
        (
            Inv::Domain,
            2,
            "edge [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0]]".to_string(),
        ),
        (Inv::Seams, 0, String::new()),
        (Inv::Embedding, 3, String::new()),
        (Inv::Names, 0, String::new()),
    ];
    for (scenario, (inv, check, item)) in expected.iter().enumerate() {
        let hooks = cube_hooks(scenario);
        let identity = build(&p, &Perm::identity(&p), BodyKind::Solid);
        let first = failure(deterministic(|| full(&identity, &hooks)));
        assert_eq!((first.inv, first.check), (*inv, *check));
        let want = describe(&identity, first.item);
        if !item.is_empty() {
            assert_eq!(&want, item);
        }
        for seed in varde_testing::seeds(16, 256) {
            let mut rng = Rng::new(seed);
            let body = build(&p, &Perm::random(&p, &mut rng), BodyKind::Solid);
            let got = failure(full(&body, &hooks));
            assert_eq!(
                (got.inv, got.check, got.finding),
                (first.inv, first.check, first.finding)
            );
            assert_eq!(describe(&body, got.item), want, "seed {seed}");
        }
    }
}

/// The coedge reported on an edge is told apart by its face and by which
/// way it runs from the edge's lesser end, not by the stored direction.
#[test]
fn coedges_are_ordered_by_their_face_and_their_way_along_the_edge() {
    let p = cube();
    let hooks = cube_hooks(1);
    let body = build(&p, &Perm::identity(&p), BodyKind::Solid);
    let got = failure(full(&body, &hooks));
    // The least top edge runs from (0,0,1) to (0,1,1); of its two
    // coedges, the one on the lesser face key: face 1 (the top) runs it
    // from (0,1,1), face 4 (`x = 0`) from (0,0,1).
    assert_eq!(
        describe(&body, got.item),
        format!(
            "coedge [0.0, 1.0, 1.0] → [0.0, 0.0, 1.0] on {:?}",
            face_key(1)
        )
    );
}

/// The invariants run in order: the first failing one is reported
/// whatever the later ones find, and within one, its kinds of item in its
/// order.
#[test]
fn the_first_failing_invariant_and_kind_is_reported() {
    let p = cube();
    let body = build(&p, &Perm::identity(&p), BodyKind::Solid);
    let any_face = |_: &Brep, item: Item| matches!(item, Item::Face(_)).then(|| fault(0)).flatten();
    let hooks = hooks(vec![
        (Inv::Joins, failing(&[ItemKind::Face], any_face)),
        (Inv::Forms, failing(&[ItemKind::Face], any_face)),
    ]);
    assert_eq!(failure(full(&body, &hooks)).inv, Inv::Joins);
    let without = run_on(
        &body,
        Scope::Full,
        Some(Inv::Joins),
        &hooks,
        Budget::DEFAULT,
    );
    assert_eq!(failure(without).inv, Inv::Forms);
    // Faces before vertices, as the hook orders them, though every
    // vertex fails too.
    let hooks = self::hooks(vec![(
        Inv::Topology,
        failing(&[ItemKind::Face, ItemKind::Vert], |_, item| match item {
            Item::Face(f) if f.0 == 5 => fault(1),
            Item::Vert(_) => fault(0),
            _ => None,
        }),
    )]);
    let got = failure(full(&body, &hooks));
    assert_eq!((got.item, got.check), (Item::Face(FaceId(5)), 1));
}

/// A sheet's check leaves out invariant 8 and runs the rest.
#[test]
fn a_sheet_is_checked_without_orientation() {
    let p = cube();
    let on_body = |inv| {
        hooks(vec![(
            inv,
            failing(&[ItemKind::Body], |_, item| {
                (item == Item::Body).then(|| fault(0)).flatten()
            }),
        )])
    };
    let solid = build(&p, &Perm::identity(&p), BodyKind::Solid);
    let sheet = build(&p, &Perm::identity(&p), BodyKind::Sheet);
    let orientation = on_body(Inv::Orientation);
    assert_eq!(failure(full(&solid, &orientation)).inv, Inv::Orientation);
    assert_eq!(full(&sheet, &orientation), Ok(()));
    for inv in Inv::ALL.into_iter().filter(|&i| i.on_sheets()) {
        assert_eq!(failure(full(&sheet, &on_body(inv))).inv, inv);
    }
}

/// `check_except` leaves out the one invariant and nothing else.
#[test]
fn one_invariant_is_left_out() {
    let p = pyramid();
    let body = build(&p, &Perm::identity(&p), BodyKind::Solid);
    for inv in Inv::ALL {
        let hooks = hooks(vec![(
            inv,
            failing(&[ItemKind::Vert], |_, item| {
                matches!(item, Item::Vert(_)).then(|| fault(0)).flatten()
            }),
        )]);
        assert_eq!(failure(full(&body, &hooks)).inv, inv);
        assert_eq!(
            run_on(&body, Scope::Full, Some(inv), &hooks, Budget::DEFAULT),
            Ok(())
        );
        for other in Inv::ALL.into_iter().filter(|&o| o != inv) {
            let r = run_on(&body, Scope::Full, Some(other), &hooks, Budget::DEFAULT);
            assert_eq!(failure(r).inv, inv);
        }
    }
}

/// After an operation, invariants 4 to 9 check only the changed faces and
/// their neighbours (by an edge or only by a vertex), their edges and
/// vertices and the changed edges and vertices; 1 to 3 and 10 check
/// everything.
#[test]
fn a_scoped_check_covers_what_changed_and_its_neighbours() {
    // The cube's top changed: its neighbours are the four walls, the
    // bottom shares nothing with it.
    let p = cube();
    let body = build(&p, &Perm::identity(&p), BodyKind::Solid);
    let face = |j: usize| {
        move |b: &Brep, item: Item| match item {
            Item::Face(f) if *b.key(f) == face_key(j) => fault(0),
            _ => None,
        }
    };
    let changed = Changed {
        faces: vec![FaceId(1)],
        ..Changed::default()
    };
    for inv in Inv::ALL {
        let on_bottom = hooks(vec![(inv, failing(&[ItemKind::Face], face(0)))]);
        let r = run_on(
            &body,
            Scope::Changed(&changed),
            None,
            &on_bottom,
            Budget::DEFAULT,
        );
        if inv.is_scoped() {
            assert_eq!(r, Ok(()), "{inv:?}");
        } else {
            assert_eq!(failure(r).inv, inv);
        }
        let on_wall = hooks(vec![(inv, failing(&[ItemKind::Face], face(2)))]);
        let r = run_on(
            &body,
            Scope::Changed(&changed),
            None,
            &on_wall,
            Budget::DEFAULT,
        );
        assert_eq!(failure(r).inv, inv);
    }
    // The pyramid's wall 1 changed: wall 3, opposite, shares only the
    // apex with it, and is a neighbour too.
    let p = pyramid();
    let body = build(&p, &Perm::identity(&p), BodyKind::Solid);
    let changed = Changed {
        faces: vec![FaceId(1)],
        ..Changed::default()
    };
    let opposite = hooks(vec![(Inv::Trims, failing(&[ItemKind::Face], face(3)))]);
    let r = run_on(
        &body,
        Scope::Changed(&changed),
        None,
        &opposite,
        Budget::DEFAULT,
    );
    assert_eq!(failure(r).item, Item::Face(FaceId(3)));
    // A new edge alone: that edge and its two vertices, nothing else.
    let e = EdgeId(0);
    let ends = body.ends(e);
    let changed = Changed {
        edges: vec![e],
        ..Changed::default()
    };
    let kinds = [
        ItemKind::Vert,
        ItemKind::Edge,
        ItemKind::Coedge,
        ItemKind::Face,
        ItemKind::Shell,
    ];
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_by_hook = seen.clone();
    let hooks = hooks(vec![(
        Inv::Embedding,
        failing(&kinds, move |_, item| {
            seen_by_hook.lock().unwrap().push(item);
            None
        }),
    )]);
    assert_eq!(
        run_on(
            &body,
            Scope::Changed(&changed),
            None,
            &hooks,
            Budget::DEFAULT
        ),
        Ok(())
    );
    let mut seen = seen.lock().unwrap().clone();
    seen.sort_by_key(|i| format!("{i:?}"));
    let mut want = vec![Item::Edge(e), Item::Vert(ends[0]), Item::Vert(ends[1])];
    want.sort_by_key(|i| format!("{i:?}"));
    assert_eq!(seen, want);
}

/// An item needing a part the build doesn't certify is refused, never
/// passed, whoever's body it is; and the body without faces passes the
/// check as built.
#[test]
fn unbuilt_parts_are_refused() {
    let p = pyramid();
    let body = build(&p, &Perm::identity(&p), BodyKind::Solid);
    let hooks = hooks(vec![(
        Inv::Trims,
        failing(&[ItemKind::Face], |_, _| {
            Some(Fault {
                check: 0,
                finding: Finding::Unbuilt,
            })
        }),
    )]);
    let err = full(&body, &hooks).unwrap_err();
    for role in [Role::Operand, Role::Constructed, Role::Result] {
        let Outcome::Error(e) = err.outcome(role) else {
            panic!("unbuilt is an error")
        };
        #[cfg(feature = "staging")]
        assert_eq!(
            e,
            KernelError::Unsupported(crate::infra::error::Unsupported::Stage(
                crate::infra::error::Stage::CheckStage
            ))
        );
        #[cfg(not(feature = "staging"))]
        assert!(matches!(e, KernelError::NotImplemented(_)));
    }
    for kind in [BodyKind::Solid, BodyKind::Sheet] {
        let empty = Brep::new(kind, status());
        assert_eq!(empty.check(&Budget::DEFAULT), Ok(()));
    }
}

/// What a failure means, by invariant, finding and role.
#[test]
fn failures_mean_what_their_role_makes_them() {
    let at = |inv, finding| {
        CheckError::Failed(CheckFailure {
            inv,
            item: Item::Body,
            check: 0,
            finding,
        })
    };
    let err = |e| Outcome::Error(e);
    let defect = err(KernelError::Internal(Internal::Inconsistent(
        Inconsistency::ResultDefect,
    )));
    let self_x = err(KernelError::Invalid(Invalid::SelfIntersection));
    for inv in Inv::ALL {
        let operand = at(inv, Finding::Defect).outcome(Role::Operand);
        assert_eq!(
            operand,
            err(KernelError::Invalid(Invalid::Check { inv: inv.number() }))
        );
        assert_eq!(at(inv, Finding::Defect).outcome(Role::Constructed), defect);
        let result = at(inv, Finding::Defect).outcome(Role::Result);
        match inv {
            Inv::Seams => assert_eq!(result, Outcome::Accuracy),
            Inv::Orientation => assert_eq!(result, Outcome::LadderSignal),
            _ => assert_eq!(result, defect),
        }
    }
    let crossing = at(Inv::Embedding, Finding::Crossing);
    assert_eq!(crossing.outcome(Role::Operand), self_x);
    assert_eq!(crossing.outcome(Role::Result), defect);
    let window = at(Inv::Embedding, Finding::EmptyWindow);
    assert_eq!(window.outcome(Role::Operand), self_x);
    assert_eq!(window.outcome(Role::Constructed), self_x);
    assert_eq!(
        window.outcome(Role::Result),
        err(KernelError::Internal(Internal::Inconsistent(
            Inconsistency::ClusterRuleGap
        )))
    );
    assert_eq!(window.inv(), Some(Inv::Embedding));
}

/// Running out while checking names the invariant, the budget's profile
/// its stage; running out while forming the order names none.
#[test]
fn running_out_names_the_invariant() {
    let p = cube();
    let body = build(&p, &Perm::identity(&p), BodyKind::Solid);
    let mut costly = hooks(Vec::new());
    costly[3].units = 1_000;
    let r = run_on(&body, Scope::Full, None, &costly, Budget::new(2_000));
    let Err(CheckError::OutOfBudget { inv, error }) = r else {
        panic!("not out of budget: {r:?}")
    };
    assert_eq!(inv, Some(Inv::Trims));
    let KernelError::OutOfBudget { cause, profile } = error else {
        panic!("not out of budget: {error:?}")
    };
    assert_eq!(cause, Cause::Work);
    assert_eq!(profile.stopped.stage, "check 4");
    let r = run_on(&body, Scope::Full, None, &costly, Budget::new(10));
    assert!(
        matches!(r, Err(CheckError::OutOfBudget { inv: None, .. })),
        "{r:?}"
    );
}

/// The invariants' numbers.
#[test]
fn invariants_are_numbered_in_order() {
    for (k, inv) in Inv::ALL.into_iter().enumerate() {
        assert_eq!(usize::from(inv.number()), k + 1);
        assert_eq!(Inv::from_number(inv.number()), Some(inv));
    }
    assert_eq!(Inv::from_number(0), None);
    assert_eq!(Inv::from_number(11), None);
}
