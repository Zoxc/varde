use std::collections::BTreeMap;

use malachite::Rational;
use varde_decide::bezier::HomPatch;
use varde_decide::count::boxes::{BoxKey, UvSpan};
use varde_decide::leaves::flat;
use varde_decide::order::{EdgeKey, EntityOrder, VertKey};
use varde_testkit::sheets::{self, Polygons};

use super::super::cluster::tests::{B_K, ext, fixture_leaves, rat, rectangle, whole, work};
use super::super::cluster::{ClusterLeaf, Tangency};
use super::super::corner::{CornerInput, Split, corner};
use super::super::rim::{Along, Element, Implicit, SnappedFace, rim};
use super::*;

const A: Operand = Operand::A;
const B: Operand = Operand::B;

fn q(x: f64) -> Rational {
    rat(x)
}

fn uv(u: f64, v: f64) -> P2 {
    [q(u), q(v)]
}

fn band(_: Along) -> f64 {
    1e-3
}

fn narrow(_: Element) -> f64 {
    1e-4
}

/// A leaf of two given boxes, Near, for rims read off boxes alone.
struct Boxes([BoxKey; 2]);

impl ClusterLeaf for Boxes {
    fn boxes(&self) -> [BoxKey; 2] {
        self.0
    }

    fn world(&self) -> [[[f64; 3]; 2]; 2] {
        [[[0.0; 3], [1.0; 3]]; 2]
    }

    fn tangency(&self) -> Option<Tangency> {
        Some(Tangency::Near)
    }
}

/// Vertices and straight edges of both operands, ordered by position.
#[derive(Default)]
struct Geom {
    verts: BTreeMap<(Operand, VertId), [f64; 3]>,
    edges: BTreeMap<(Operand, EdgeId), [VertId; 2]>,
}

impl EntityOrder for Geom {
    type Key = ();

    fn ends(&self, o: Operand, g: EdgeId) -> [VertId; 2] {
        self.edges[&(o, g)]
    }

    fn vert_key(&self, o: Operand, v: VertId) -> VertKey<()> {
        VertKey::new(self.verts[&(o, v)], []).unwrap()
    }

    fn edge_key(&self, o: Operand, g: EdgeId) -> EdgeKey<()> {
        let [a, b] = self.ends(o, g);
        let (p, r) = (self.verts[&(o, a)], self.verts[&(o, b)]);
        let mid = [0, 1, 2].map(|k| 0.5 * (p[k] + r[k]));
        EdgeKey::new([self.vert_key(o, a), self.vert_key(o, b)], mid, [(), ()]).unwrap()
    }
}

/// The straight span from `a` to `b`, weights 1.
fn span(a: [f64; 2], b: [f64; 2]) -> UvSpan {
    let m = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
    [[a[0], a[1], 1.0], [m[0], m[1], 1.0], [b[0], b[1], 1.0]]
}

const KNOTS: [f64; 2] = [0.0, 1.0];

/// The scale of the bowl's height, `2⁻¹⁵` mm per mm².
const C: f64 = 1.0 / 32768.0;

/// The bowl `z = c·((x − 5)² + (y − 5)²) − h` over `x, y ∈ [0, 10]`, a
/// biquadratic patch exactly (`u` along `x`, `v` along `y`), its zero
/// circle of radius 3 about `(5, 5)` at `h = 9c`.
fn bowl() -> HomPatch<Rational> {
    let h = 9.0 * C;
    let bern = [25.0, -25.0, 25.0];
    let xs = [0.0, 5.0, 10.0];
    let net = (0..9).map(|n| {
        let (i, j) = (n / 3, n % 3);
        let z = q(C) * (q(bern[i]) + q(bern[j])) - q(h);
        [q(xs[i]), q(xs[j]), z, Rational::from(1)]
    });
    HomPatch::new(2, 2, net).expect("a biquadratic net")
}

const FLOOR: Implicit = Implicit::Plane {
    normal: [0.0, 0.0, 1.0],
    offset: 0.0,
};

/// The ids a bowl fixture uses, offset so that none is a small number.
#[derive(Clone, Copy)]
struct Ids {
    /// The bowl's face (operand `B`).
    face: u32,
    /// Its four edges, `edge..edge + 4`, and vertices.
    edge: u32,
    vert: u32,
    /// The floor's faces (operand `A`), `target`, `target + 1`.
    target: u32,
    /// The floor's edge between them and its vertices.
    cut: u32,
    cut_vert: u32,
}

const SMALL: Ids = Ids {
    face: 0,
    edge: 0,
    vert: 0,
    target: 0,
    cut: 7,
    cut_vert: 10,
};

const LARGE: Ids = Ids {
    face: 40,
    edge: 50,
    vert: 60,
    target: 70,
    cut: 80,
    cut_vert: 90,
};

/// The bowl face's own trims: the square `[0, 1]²` counter-clockwise,
/// edge `k` from vertex `k` to `k + 1`, rotated to start at `start`.
struct Square {
    spans: Vec<UvSpan>,
    ids: Ids,
    start: usize,
}

impl Square {
    fn new(ids: Ids, start: usize) -> Square {
        let c = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        Square {
            spans: (0..4).map(|k| span(c[k], c[(k + 1) % 4])).collect(),
            ids,
            start,
        }
    }

    fn trims(&self) -> Vec<RimTrim<'_>> {
        (0..4)
            .map(|i| {
                let k = (i + self.start) % 4;
                RimTrim {
                    edge: EdgeId(self.ids.edge + k as u32),
                    ends: [
                        VertId(self.ids.vert + k as u32),
                        VertId(self.ids.vert + ((k + 1) % 4) as u32),
                    ],
                    spans: &self.spans[k..k + 1],
                    knots: &KNOTS,
                    left: true,
                }
            })
            .collect()
    }
}

/// The four boxes about the bowl's bottom, `[2.5, 7.5]²`, each against
/// the floor's faces `targets` (by offset from the first) on its side of
/// `u = 0.5` (left, right).
fn centre_boxes(ids: Ids, left: &[u32], right: &[u32]) -> Vec<Boxes> {
    let e = [ext(1, 2, 2), ext(2, 3, 2)];
    let mut out = Vec::new();
    for (i, &u) in e.iter().enumerate() {
        for &v in &e {
            let s = BoxKey::new(B, FaceId(ids.face), [0, 0], u, v);
            for &t in if i == 0 { left } else { right } {
                out.push(Boxes([whole(A, ids.target + t, [0, 0]), s]));
            }
        }
    }
    out
}

/// The positions of the bowl's square's corners and of the floor's cut
/// edge's ends.
fn bowl_geom(ids: Ids) -> Geom {
    let mut g = Geom::default();
    let c = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    for (k, p) in c.iter().enumerate() {
        let z = C * ((p[0] - 5.0) * (p[0] - 5.0) + (p[1] - 5.0) * (p[1] - 5.0)) - 9.0 * C;
        g.verts
            .insert((B, VertId(ids.vert + k as u32)), [p[0], p[1], z]);
    }
    for k in 0..4u32 {
        g.edges.insert(
            (B, EdgeId(ids.edge + k)),
            [VertId(ids.vert + k), VertId(ids.vert + (k + 1) % 4)],
        );
    }
    g.verts.insert((A, VertId(ids.cut_vert)), [6.25, 0.0, 0.0]);
    g.verts
        .insert((A, VertId(ids.cut_vert + 1)), [6.25, 10.0, 0.0]);
    g.edges.insert(
        (A, EdgeId(ids.cut)),
        [VertId(ids.cut_vert), VertId(ids.cut_vert + 1)],
    );
    g
}

/// The pipeline on one face: rim, corner rule, rewrite.
fn run_all<L: ClusterLeaf>(
    leaves: &[L],
    faces: &[SnappedFace<'_>],
    targets: &[(FaceId, Implicit)],
    geom: &Geom,
    cuts: &[TargetCut<'_>],
    edit: impl FnOnce(&mut Corner),
) -> (Rim, Result<Rewrite, RewriteError>) {
    let input = RimInput {
        snapped: B,
        faces,
        targets,
        band: &band,
    };
    let r = rim(leaves, &input, &mut work()).expect("a rim");
    let ci = CornerInput {
        b_k: B_K,
        order: geom,
        band: &narrow,
        candidates: &[],
        fixed: &[],
        groups: &[],
        coincident: &[],
    };
    let mut c = corner(&r, &input, &ci, &mut work()).expect("the corner rule");
    edit(&mut c);
    let out = rewrite(&r, &input, &c, cuts, &mut work());
    (r, out)
}

fn edge(r: &Rewrite, k: SnapEdge) -> &NewEdge {
    r.edges
        .iter()
        .find(|e| e.key == k)
        .unwrap_or_else(|| panic!("edge {k:?} in {:?}", r.edges))
}

/// Each edge's uses, `(face, forward)`.
fn uses(r: &Rewrite) -> BTreeMap<SnapEdge, Vec<(SnapFace, bool)>> {
    let mut out: BTreeMap<SnapEdge, Vec<(SnapFace, bool)>> = BTreeMap::new();
    for f in &r.faces {
        for l in &f.loops {
            for u in l {
                out.entry(u.edge).or_default().push((f.key, u.forward));
            }
        }
    }
    out
}

/// Every loop closes: each use ends where the next starts.
fn assert_closed(r: &Rewrite) {
    for f in &r.faces {
        for l in &f.loops {
            for (i, u) in l.iter().enumerate() {
                let n = &l[(i + 1) % l.len()];
                let at = |u: &Use, end: bool| {
                    let e = edge(r, u.edge);
                    e.ends[usize::from(end == u.forward)]
                };
                assert_eq!(at(u, true), at(n, false), "{:?} in {f:?}", u.edge);
            }
        }
    }
}

#[test]
fn the_bowl_s_face_splits_into_a_kept_ring_and_a_snapped_square() {
    // The bowl's cluster is the square `[0.25, 0.75]²` of its face: the
    // face keeps the ring outside it, the square goes onto the floor. The
    // rim's four pieces are cut at the eight branch vertices into twelve
    // rim edges, each used once by each face, opposite ways.
    let ids = SMALL;
    let tiles = [([0, 0], bowl())];
    let sq = Square::new(ids, 0);
    let own = sq.trims();
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &own,
        target: &[],
    }];
    let leaves = centre_boxes(ids, &[0], &[0]);
    let (rm, out) = run_all(
        &leaves,
        &faces,
        &[(FaceId(0), FLOOR)],
        &bowl_geom(ids),
        &[],
        |_| {},
    );
    let r = out.unwrap();
    assert_eq!(rm.pieces.len(), 4);
    let keys: Vec<SnapFace> = r.faces.iter().map(|f| f.key).collect();
    let (kept, snapped) = (
        SnapFace::Kept {
            face: FaceId(0),
            part: 0,
        },
        SnapFace::Snapped {
            face: FaceId(0),
            target: FaceId(0),
            part: 0,
        },
    );
    assert_eq!(keys, [kept, snapped]);
    assert_eq!(r.faces[0].on, (B, FaceId(0)));
    assert_eq!(r.faces[1].on, (A, FaceId(0)));
    // The ring: the square's four edges whole, and the hole.
    let ring = &r.faces[0].loops;
    assert_eq!(ring.len(), 2);
    let outer: Vec<Use> = (0..4)
        .map(|k| Use {
            edge: SnapEdge::Piece {
                edge: EdgeId(k),
                part: 0,
            },
            forward: true,
        })
        .collect();
    assert_eq!(ring[0], outer);
    assert_eq!(ring[1].len(), 12);
    assert!(ring[1].iter().all(|u| !u.forward));
    let square = &r.faces[1].loops;
    assert_eq!(square.len(), 1);
    assert_eq!(square[0].len(), 12);
    assert!(square[0].iter().all(|u| u.forward));
    // Every rim edge once in each face, opposite ways.
    for (e, us) in uses(&r) {
        match e {
            SnapEdge::Rim { .. } => {
                assert_eq!(us, [(kept, false), (snapped, true)], "{e:?}");
            }
            SnapEdge::Piece { .. } => assert_eq!(us, [(kept, true)]),
            SnapEdge::Cut { .. } => panic!("{e:?}"),
        }
    }
    assert_closed(&r);
    // Vertices: the square's corners, the rim's four corners (box sides
    // only) and its eight branch vertices, two per piece.
    let mut kinds = [0; 3];
    for v in &r.verts {
        match v.key {
            SnapVert::Vert(B, _) => kinds[0] += 1,
            SnapVert::RimBox { face, .. } => {
                assert_eq!(face, FaceId(0));
                kinds[1] += 1;
            }
            SnapVert::Branch { index, .. } => {
                assert!(index < 2);
                kinds[2] += 1;
            }
            k => panic!("{k:?}"),
        }
        assert_eq!(v.moved, Moved::STAYS);
    }
    assert_eq!(kinds, [4, 4, 8]);
    // The map: the snapped square's pre-image is the cluster's square.
    let pre = &r.map.faces[&snapped];
    assert_eq!(pre.parent, (B, Entity::Face(FaceId(0))));
    let PreImage::Region(loops) = &pre.at else {
        panic!("{pre:?}");
    };
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0][0], uv(0.25, 0.25));
    for c in [uv(0.75, 0.25), uv(0.75, 0.75), uv(0.25, 0.75)] {
        assert!(loops[0].contains(&c));
    }
    let corner0 = SnapVert::RimBox {
        face: FaceId(0),
        piece: 0,
    };
    assert_eq!(
        r.map.verts[&corner0],
        Pre {
            parent: (B, Entity::Face(FaceId(0))),
            at: PreImage::Point(uv(0.25, 0.25)),
        }
    );
    assert_eq!(
        r.map.edges[&SnapEdge::Piece {
            edge: EdgeId(1),
            part: 0
        }],
        Pre {
            parent: (B, Entity::Edge(EdgeId(1))),
            at: PreImage::Range([q(0.0), q(1.0)]),
        }
    );
}

/// The bowl's cluster split by the floor's edge at `u = 0.625` between
/// its faces: boxes left of `u = 0.5` against the left face, right of it
/// against both. With `sides` the cut's faces left and right.
fn cut_fixture(
    ids: Ids,
    start: usize,
    sides: [u32; 2],
    extra: bool,
) -> Result<Rewrite, RewriteError> {
    let tiles = [([0, 0], bowl())];
    let sq = Square::new(ids, start);
    let own = sq.trims();
    let faces = [SnappedFace {
        face: FaceId(ids.face),
        tiles: &tiles,
        own: &own,
        target: &[],
    }];
    let right: &[u32] = if sides[0] == sides[1] { &[0] } else { &[0, 1] };
    let leaves = centre_boxes(ids, &[0], right);
    let cut_span = [span([0.625, 0.0], [0.625, 1.0])];
    let mut cuts = vec![TargetCut {
        face: FaceId(ids.face),
        edge: EdgeId(ids.cut),
        ends: [VertId(ids.cut_vert), VertId(ids.cut_vert + 1)],
        spans: &cut_span,
        knots: &KNOTS,
        sides: sides.map(|s| FaceId(ids.target + s)),
    }];
    let other_span = [span([0.1, 0.0], [0.1, 1.0])];
    if extra {
        // A target edge carried into a face the cluster doesn't reach.
        cuts.insert(
            0,
            TargetCut {
                face: FaceId(ids.face + 1),
                edge: EdgeId(ids.cut + 1),
                ends: [VertId(ids.cut_vert + 2), VertId(ids.cut_vert + 3)],
                spans: &other_span,
                knots: &KNOTS,
                sides: [FaceId(ids.target), FaceId(ids.target + 1)],
            },
        );
    }
    let targets = [(FaceId(ids.target), FLOOR), (FaceId(ids.target + 1), FLOOR)];
    run_all(&leaves, &faces, &targets, &bowl_geom(ids), &cuts, |_| {}).1
}

#[test]
fn inner_pieces_split_along_a_target_edge_between_two_target_faces() {
    // The floor's edge at `u = 0.625` crosses the cluster: the square goes
    // onto the floor as two faces, one each side, sharing one piece of
    // that edge; its parts outside the cluster split nothing (the bowl's
    // own edges stay whole).
    let r = cut_fixture(SMALL, 0, [0, 1], false).unwrap();
    let f = FaceId(0);
    let keys: Vec<SnapFace> = r.faces.iter().map(|f| f.key).collect();
    let snapped = |t: u32| SnapFace::Snapped {
        face: f,
        target: FaceId(t),
        part: 0,
    };
    assert_eq!(
        keys,
        [SnapFace::Kept { face: f, part: 0 }, snapped(0), snapped(1)]
    );
    assert_eq!(r.faces[2].on, (A, FaceId(1)));
    let cut = SnapEdge::Cut {
        face: f,
        edge: EdgeId(7),
        part: 0,
    };
    let e = edge(&r, cut);
    // Its ends: where it crosses the rim's bottom and top pieces, inside
    // them (those pieces run from `u = 0.5` to `u = 0.75`).
    let mut crossed = Vec::new();
    for v in e.ends {
        let SnapVert::CutRim { cut: c, piece } = v else {
            panic!("{v:?}");
        };
        assert_eq!(c, EdgeId(7));
        crossed.push(piece);
    }
    assert_ne!(crossed[0], crossed[1]);
    for (v, t) in e.ends.iter().zip([0.25, 0.75]) {
        assert_eq!(
            r.map.verts[v],
            Pre {
                parent: (A, Entity::Edge(EdgeId(7))),
                at: PreImage::Param(q(t)),
            }
        );
    }
    assert_eq!(
        r.map.edges[&cut],
        Pre {
            parent: (A, Entity::Edge(EdgeId(7))),
            at: PreImage::Range([q(0.25), q(0.75)]),
        }
    );
    // The cut runs up `u = 0.625`: the left face (target 0) has it on its
    // left, so runs it forward; the right one backward.
    let u = uses(&r);
    assert_eq!(u[&cut], [(snapped(0), true), (snapped(1), false)]);
    assert!(
        r.edges
            .iter()
            .filter(|e| matches!(e.key, SnapEdge::Cut { .. }))
            .count()
            == 1
    );
    for e in &r.edges {
        if let SnapEdge::Piece { part, .. } = e.key {
            assert_eq!(part, 0, "{e:?}");
        }
    }
    assert_closed(&r);
}

#[test]
fn a_target_edge_with_one_face_on_both_sides_splits_nothing() {
    // A slit of the floor: the square goes onto it as one face.
    let r = cut_fixture(SMALL, 0, [0, 0], false).unwrap();
    let keys: Vec<SnapFace> = r.faces.iter().map(|f| f.key).collect();
    assert_eq!(
        keys,
        [
            SnapFace::Kept {
                face: FaceId(0),
                part: 0
            },
            SnapFace::Snapped {
                face: FaceId(0),
                target: FaceId(0),
                part: 0
            }
        ]
    );
    assert!(
        r.edges
            .iter()
            .all(|e| !matches!(e.key, SnapEdge::Cut { .. }))
    );
}

#[test]
fn keys_are_made_of_operand_ids_never_fresh_ones() {
    // With every operand id large, each key holds only ids of the
    // operands' entities given (the rest of a key is a rim piece index or
    // an ordinal along its parent, never an index into the rewrite); the
    // keys are the same whatever order the input lists things in, and a
    // target edge on a face the cluster doesn't reach changes nothing.
    let ids = LARGE;
    let r = cut_fixture(ids, 0, [0, 1], false).unwrap();
    let mut given: BTreeSet<(Operand, Entity)> = BTreeSet::new();
    given.insert((B, FaceId(ids.face).into()));
    for k in 0..4 {
        given.insert((B, EdgeId(ids.edge + k).into()));
        given.insert((B, VertId(ids.vert + k).into()));
    }
    for t in 0..2 {
        given.insert((A, FaceId(ids.target + t).into()));
        given.insert((A, VertId(ids.cut_vert + t).into()));
    }
    given.insert((A, EdgeId(ids.cut).into()));
    let mut seen = 0;
    for v in &r.verts {
        for id in v.key.operand_ids(B) {
            assert!(given.contains(&id), "{:?} in {:?}", id, v.key);
            seen += 1;
        }
    }
    for e in &r.edges {
        for id in e.key.operand_ids(B) {
            assert!(given.contains(&id), "{:?} in {:?}", id, e.key);
            seen += 1;
        }
    }
    for f in &r.faces {
        for id in f.key.operand_ids(B) {
            assert!(given.contains(&id), "{:?} in {:?}", id, f.key);
            seen += 1;
        }
        assert!(r.map.faces.contains_key(&f.key));
    }
    assert!(seen > 30);
    // Every parent is an operand entity given.
    for pre in r
        .map
        .verts
        .values()
        .chain(r.map.edges.values())
        .chain(r.map.faces.values())
    {
        assert!(given.contains(&pre.parent), "{pre:?}");
    }
    // The input in other orders, with an unrelated target edge.
    for start in 1..4 {
        assert_eq!(cut_fixture(ids, start, [0, 1], true).unwrap(), r, "{start}");
    }
    // The same keys as at small ids, up to the renaming.
    let small = cut_fixture(SMALL, 2, [0, 1], false).unwrap();
    assert_eq!(small.faces.len(), r.faces.len());
    assert_eq!(small.edges.len(), r.edges.len());
    assert_eq!(small.verts.len(), r.verts.len());
}

/// A face's own trims on its rectangle tile: per edge of its loop, the
/// straight span between its vertices' UVs.
struct OwnTrims {
    spans: Vec<UvSpan>,
    edges: Vec<(EdgeId, [VertId; 2], bool)>,
}

fn fl(x: &Rational) -> f64 {
    use malachite::base::num::conversion::traits::RoundingFrom;
    use malachite::base::rounding_modes::RoundingMode::Nearest;
    f64::rounding_from(x, Nearest).0
}

fn own_trims(body: &Polygons, f: usize, tile: &HomPatch<Rational>, flipped: bool) -> OwnTrims {
    let net = tile.tensors();
    let ctl = |i: usize, j: usize| [0, 1, 2].map(|k| net[k].get(i, j).clone());
    let (p0, pv, pu) = (ctl(0, 0), ctl(0, 1), ctl(1, 0));
    let dot = |a: &[Rational; 3], b: &[Rational; 3]| {
        (0..3).fold(Rational::from(0), |s, k| s + &a[k] * &b[k])
    };
    let d = |a: &[Rational; 3], b: &[Rational; 3]| [0, 1, 2].map(|k| &a[k] - &b[k]);
    let (du, dv) = (d(&pu, &p0), d(&pv, &p0));
    let uv_of = |v: u32| {
        let p = body.verts[v as usize].map(q);
        let r = d(&p, &p0);
        [
            fl(&(dot(&r, &du) / dot(&du, &du))),
            fl(&(dot(&r, &dv) / dot(&dv, &dv))),
        ]
    };
    let edges = body.edges();
    let l = &body.faces[f];
    let mut out = OwnTrims {
        spans: Vec::new(),
        edges: Vec::new(),
    };
    for k in 0..l.len() {
        let (a, b) = (l[k], l[(k + 1) % l.len()]);
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let id = edges
            .iter()
            .position(|(e, _)| *e == [lo, hi])
            .expect("an edge");
        out.spans.push(span(uv_of(lo), uv_of(hi)));
        out.edges.push((
            EdgeId(id as u32),
            [VertId(lo), VertId(hi)],
            (a == lo) != flipped,
        ));
    }
    out
}

fn trims_of(o: &OwnTrims) -> Vec<RimTrim<'_>> {
    o.edges
        .iter()
        .enumerate()
        .map(|(k, (e, ends, left))| RimTrim {
            edge: *e,
            ends: *ends,
            spans: &o.spans[k..k + 1],
            knots: &KNOTS,
            left: *left,
        })
        .collect()
}

/// A face's plane, outward.
fn plane_of(body: &Polygons, f: usize) -> Implicit {
    let l = &body.faces[f];
    let mut n = [0.0; 3];
    for k in 0..l.len() {
        let (a, b) = (
            body.verts[l[k] as usize],
            body.verts[l[(k + 1) % l.len()] as usize],
        );
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let p = body.verts[l[0] as usize];
    Implicit::Plane {
        normal: n,
        offset: n[0] * p[0] + n[1] * p[1] + n[2] * p[2],
    }
}

/// The sheet folded flush over the box's edge, rewritten, with `edit` to
/// the corner rule's decisions.
fn fold(edit: impl FnOnce(&mut Corner)) -> Result<Rewrite, RewriteError> {
    let pair = sheets::box_edge_fold(0.0);
    let ls: Vec<flat::Leaf> = fixture_leaves(&pair, rectangle, 2)
        .into_iter()
        .filter(|l| l.tangency() == Some(Tangency::Near))
        .collect();
    let tiles: Vec<[([i64; 2], HomPatch<Rational>); 1]> = (0..2)
        .map(|f| [([0, 0], rectangle(&pair.b, f).0)])
        .collect();
    let owns: Vec<OwnTrims> = (0..2)
        .map(|f| {
            let (t, flipped) = rectangle(&pair.b, f);
            own_trims(&pair.b, f, &t, flipped)
        })
        .collect();
    let trims: Vec<Vec<RimTrim<'_>>> = owns.iter().map(trims_of).collect();
    let faces: Vec<SnappedFace<'_>> = (0..2)
        .map(|f| SnappedFace {
            face: FaceId(f as u32),
            tiles: &tiles[f],
            own: &trims[f],
            target: &[],
        })
        .collect();
    let targets: Vec<(FaceId, Implicit)> = (0..pair.a.faces.len())
        .map(|f| (FaceId(f as u32), plane_of(&pair.a, f)))
        .collect();
    let mut g = Geom::default();
    for (v, p) in pair.b.verts.iter().enumerate() {
        g.verts.insert((B, VertId(v as u32)), *p);
    }
    for (e, (ends, _)) in pair.b.edges().iter().enumerate() {
        g.edges.insert((B, EdgeId(e as u32)), ends.map(VertId));
    }
    run_all(&ls, &faces, &targets, &g, &[], edit).1
}

#[test]
fn a_fold_flush_on_a_box_edge_goes_onto_both_faces_sharing_the_fold() {
    // Both faces of the sheet lie in the cluster: each goes onto its box
    // face whole, the top onto the box's top (face 1), the front onto its
    // front (face 2); the fold edge is a snapped edge both use, opposite
    // ways.
    let r = fold(|_| {}).unwrap();
    let keys: Vec<SnapFace> = r.faces.iter().map(|f| f.key).collect();
    let (top, front) = (
        SnapFace::Snapped {
            face: FaceId(0),
            target: FaceId(1),
            part: 0,
        },
        SnapFace::Snapped {
            face: FaceId(1),
            target: FaceId(2),
            part: 0,
        },
    );
    assert_eq!(keys, [top, front]);
    let fold_edge = SnapEdge::Piece {
        edge: EdgeId(0),
        part: 0,
    };
    let u = uses(&r);
    let mut f = u[&fold_edge].clone();
    f.sort();
    assert_eq!(f.len(), 2);
    assert_eq!((f[0].0, f[1].0), (top, front));
    assert_ne!(f[0].1, f[1].1);
    assert_eq!(
        edge(&r, fold_edge).ends,
        [SnapVert::Vert(B, VertId(0)), SnapVert::Vert(B, VertId(1))]
    );
    for l in r.faces.iter().map(|f| &f.loops) {
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].len(), 4);
    }
    // The sheet's other edges once each, all whole.
    assert_eq!(r.edges.len(), 7);
    assert!(u.values().filter(|v| v.len() == 1).count() == 6);
    assert_eq!(r.verts.len(), 6);
    assert_closed(&r);
}

#[test]
fn a_corner_rule_split_cuts_an_edge_on_every_face_it_bounds() {
    // An anchor on the fold edge's middle (as a target vertex there would
    // be): the edge is cut there on both faces, at one vertex keyed by the
    // edge and the anchor, which the corner rule sends to the anchor.
    let at = (A, VertId(99));
    let pos = [20.0, 0.0, 20.0];
    let r = fold(|c| {
        c.splits.push(Split {
            edge: EdgeId(0),
            at,
            s: 0.5,
            pos,
            displacement: 1e-9,
        });
    })
    .unwrap();
    let split = SnapVert::Split {
        edge: EdgeId(0),
        at,
    };
    let pieces: Vec<&NewEdge> = r
        .edges
        .iter()
        .filter(|e| {
            matches!(
                e.key,
                SnapEdge::Piece {
                    edge: EdgeId(0),
                    ..
                }
            )
        })
        .collect();
    assert_eq!(pieces.len(), 2);
    assert_eq!(pieces[0].ends, [SnapVert::Vert(B, VertId(0)), split]);
    assert_eq!(pieces[1].ends, [split, SnapVert::Vert(B, VertId(1))]);
    let u = uses(&r);
    for p in &pieces {
        assert_eq!(u[&p.key].len(), 2, "{p:?}");
    }
    for f in &r.faces {
        assert_eq!(f.loops[0].len(), 5);
    }
    let v = r.verts.iter().find(|v| v.key == split).unwrap();
    assert_eq!(v.moved.to, Placement::Vert(A, VertId(99)));
    assert_eq!(v.moved.pos, Some(pos));
    assert_eq!(
        r.map.verts[&split],
        Pre {
            parent: (B, Entity::Edge(EdgeId(0))),
            at: PreImage::Param(q(0.5)),
        }
    );
    assert_closed(&r);
}

#[test]
fn where_the_cluster_reaches_a_target_edge_the_rim_vertices_lie_on_it() {
    // The floor ends at `u = 0.625` across the cluster: the snapped piece
    // ends there, its rim edge along the floor's edge, its ends rim
    // vertices on that edge at its parameters `0.25` and `0.75`.
    let ids = SMALL;
    let tiles = [([0, 0], bowl())];
    let sq = Square::new(ids, 0);
    let own = sq.trims();
    let c = [[0.0, 0.0], [0.625, 0.0], [0.625, 1.0], [0.0, 1.0]];
    let spans: Vec<UvSpan> = (0..4).map(|k| span(c[k], c[(k + 1) % 4])).collect();
    let target: Vec<RimTrim<'_>> = (0..4)
        .map(|k| RimTrim {
            edge: EdgeId(20 + k as u32),
            ends: [VertId(20 + k as u32), VertId(20 + ((k + 1) % 4) as u32)],
            spans: &spans[k..k + 1],
            knots: &KNOTS,
            left: true,
        })
        .collect();
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &own,
        target: &target,
    }];
    let mut g = bowl_geom(ids);
    for (k, p) in c.iter().enumerate() {
        g.verts
            .insert((A, VertId(20 + k as u32)), [10.0 * p[0], 10.0 * p[1], 0.0]);
        g.edges.insert(
            (A, EdgeId(20 + k as u32)),
            [VertId(20 + k as u32), VertId(20 + ((k + 1) % 4) as u32)],
        );
    }
    let leaves = centre_boxes(ids, &[0], &[0]);
    let (rm, out) = run_all(&leaves, &faces, &[(FaceId(0), FLOOR)], &g, &[], |_| {});
    let r = out.unwrap();
    let k = rm
        .pieces
        .iter()
        .position(|p| p.along == Along::Target(EdgeId(21)))
        .unwrap() as u32;
    let along = SnapEdge::Rim {
        face: FaceId(0),
        piece: k,
        part: 0,
    };
    let e = edge(&r, along);
    let next = rm.loops[0][(rm.loops[0].iter().position(|&p| p == k).unwrap() + 1) % 4];
    assert_eq!(
        e.ends,
        [
            SnapVert::RimTarget {
                edge: EdgeId(21),
                piece: k
            },
            SnapVert::RimTarget {
                edge: EdgeId(21),
                piece: next
            }
        ]
    );
    for (v, t) in e.ends.iter().zip([0.25, 0.75]) {
        assert_eq!(
            r.map.verts[v],
            Pre {
                parent: (A, Entity::Edge(EdgeId(21))),
                at: PreImage::Param(q(t)),
            }
        );
    }
    let u = uses(&r);
    assert_eq!(u[&along].len(), 2);
    assert_eq!(r.faces.len(), 2);
    assert_closed(&r);
}

#[test]
fn bad_input_is_refused() {
    let ids = SMALL;
    let tiles = [([0, 0], bowl())];
    let sq = Square::new(ids, 0);
    let own = sq.trims();
    let leaves = centre_boxes(ids, &[0], &[0]);
    let targets = [(FaceId(0), FLOOR)];
    let geom = bowl_geom(ids);
    // A face without its own trims.
    let bare = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &[],
        target: &[],
    }];
    let (_, out) = run_all(&leaves, &bare, &targets, &geom, &[], |_| {});
    assert_eq!(out, Err(RewriteError::Face(FaceId(0))));
    let faces = [SnappedFace {
        face: FaceId(0),
        tiles: &tiles,
        own: &own,
        target: &[],
    }];
    // Rim vertices that don't fit the rim.
    let (_, out) = run_all(&leaves, &faces, &targets, &geom, &[], |c| {
        c.rim_verts.pop();
    });
    assert_eq!(out, Err(RewriteError::Corner));
    // A target edge ending inside the cluster.
    let half = [span([0.625, 0.0], [0.625, 0.5])];
    let cut = [TargetCut {
        face: FaceId(0),
        edge: EdgeId(7),
        ends: [VertId(10), VertId(11)],
        spans: &half,
        knots: &KNOTS,
        sides: [FaceId(0), FaceId(1)],
    }];
    let two = [(FaceId(0), FLOOR), (FaceId(1), FLOOR)];
    let (_, out) = run_all(&leaves, &faces, &two, &geom, &cut, |_| {});
    assert_eq!(out, Err(RewriteError::Cells(FaceId(0))));
}
