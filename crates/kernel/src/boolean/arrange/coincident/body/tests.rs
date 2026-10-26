//! A box's top face realised on its body: its coedges as lines with the
//! face inside winding 1; a second box standing on half of it, its
//! footprint's arcs identified with the top's edges, carrying the join
//! across the opposed coincidence and their sources' `touch`; a seam
//! cone's apex seen at two points of its side; each refusal of the
//! input; and running out of budget.

use super::*;
use crate::build::primitives::{Primitive, build};
use crate::infra::budget::{Budget, Work};
use crate::infra::ops::{FeatureId, OpCtx, Tolerance};
use crate::topo::brep::{EdgeId, EdgeKind};
use crate::topo::build::Builder;

fn built(p: &Primitive) -> Brep {
    let ctx = OpCtx::new(FeatureId(1));
    build(p, ctx, Tolerance::DEFAULT, &mut Work::new(&Budget::DEFAULT))
        .unwrap_or_else(|e| panic!("{p:?}: {e:?}"))
}

fn cuboid(min: [f64; 3], size: [f64; 3]) -> Brep {
    built(&Primitive::Cuboid { min, size })
}

/// The face all of whose vertices have `z` = `z`.
fn face_at_z(b: &Brep, z: f64) -> FaceId {
    (0..b.face_count() as u32)
        .map(FaceId)
        .find(|&f| {
            b.loops(f)
                .flat_map(|l| b.loop_coedges(l))
                .all(|c| b.ends(b.edge(c)).iter().all(|&v| b.pos(v)[2] == z))
        })
        .expect("a face at z")
}

/// The edge between the vertices at `p` and `q`.
fn edge_between(b: &Brep, p: [f64; 3], q: [f64; 3]) -> EdgeId {
    (0..b.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| {
            let [u, v] = b.ends(e).map(|v| b.pos(v));
            (u == p && v == q) || (u == q && v == p)
        })
        .expect("an edge")
}

/// The coedge of face `f` on the edge between `p` and `q`.
fn coedge_between(b: &Brep, f: FaceId, p: [f64; 3], q: [f64; 3]) -> CoedgeId {
    let e = edge_between(b, p, q);
    b.loops(f)
        .flat_map(|l| b.loop_coedges(l))
        .find(|&c| b.edge(c) == e)
        .expect("a coedge of the face")
}

/// The face vertex at the start of `c`'s p-curve if the vertex there is
/// at `p`, else at its end.
fn end_at(b: &Brep, c: CoedgeId, p: [f64; 3]) -> VertRef {
    let end = b.pos(b.ends(b.edge(c))[0]) != p;
    VertRef::Face { coedge: c, end }
}

/// Whether `c`'s p-curve starts at the vertex at `p`.
fn starts_at(b: &Brep, c: CoedgeId, p: [f64; 3]) -> bool {
    b.pos(b.ends(b.edge(c))[0]) == p
}

/// The middle of `c`'s edge's range.
fn mid(b: &Brep, c: CoedgeId) -> f64 {
    let [t0, t1] = b.t_range(b.edge(c));
    (t0 + t1) / 2.0
}

fn run(stores: [&Brep; 2], input: &FaceArcs<'_>) -> Result<Realised, FaceError> {
    realise_on(stores, input, &mut Work::new(&Budget::DEFAULT))
}

#[test]
fn a_face_alone_winds_one_inside() {
    let a = cuboid([0.0; 3], [2.0; 3]);
    let top = face_at_z(&a, 2.0);
    let input = FaceArcs {
        operand: Operand::A,
        face: top,
        records: &[],
        lines: &[],
        arcs: &[],
    };
    let got = run([&a, &a], &input).unwrap();
    assert_eq!(got.lines.len(), 4);
    assert_eq!(got.face_uvs.len(), 4);
    assert_eq!(got.coincident.points.len(), 4);
    assert!(got.lines.iter().all(|l| matches!(l, LineRef::Coedge(_))));
    let windings: Vec<i64> = got
        .coincident
        .arrangement
        .regions()
        .iter()
        .map(|r| r.winding)
        .collect();
    assert_eq!(windings, [0, 1]);
    // Every bundle is a reused piece of its own edge, alone.
    for (bd, l) in got.bundles.iter().zip(&got.lines) {
        let LineRef::Coedge(c) = *l else {
            unreachable!()
        };
        assert_eq!(bd.sources, [(Operand::A, a.edge(c))]);
        assert!(!bd.identified && !bd.opposed_join && !bd.touch);
    }
}

/// A box `[0, 2]³` and a box `[0, 2] × [0, 1] × [2, 3]` standing on half
/// its top: the footprint's arcs on the top face, from `B`'s sides
/// against the top across the opposed coincidence of the top and `B`'s
/// bottom (`opposed`), each with `B`'s bottom edge it runs along as its
/// source; `B`'s bottom edge on `y = 0` touching when `touch`.
struct Tab {
    a: Brep,
    b: Brep,
    top: FaceId,
    /// The top's coedges on `y = 0`, `x = 0`, `x = 2`.
    y0: CoedgeId,
    x0: CoedgeId,
    x2: CoedgeId,
    /// `B`'s bottom edges on `y = 0`, `x = 0`, `x = 2`, `y = 1`.
    b_edges: [EdgeId; 4],
}

impl Tab {
    fn new(touch: bool) -> Tab {
        let a = cuboid([0.0; 3], [2.0; 3]);
        let mut b = cuboid([0.0, 0.0, 2.0], [2.0, 1.0, 1.0]);
        let p = |x, y, z| [x, y, z];
        let b_edges = [
            edge_between(&b, p(0.0, 0.0, 2.0), p(2.0, 0.0, 2.0)),
            edge_between(&b, p(0.0, 0.0, 2.0), p(0.0, 1.0, 2.0)),
            edge_between(&b, p(2.0, 0.0, 2.0), p(2.0, 1.0, 2.0)),
            edge_between(&b, p(0.0, 1.0, 2.0), p(2.0, 1.0, 2.0)),
        ];
        if touch {
            let mut ed = Builder::edit(b);
            ed.set_edge_kind(b_edges[0], EdgeKind::SameForm { touch: true });
            b = ed.finish().unwrap().0;
        }
        let top = face_at_z(&a, 2.0);
        let y0 = coedge_between(&a, top, p(0.0, 0.0, 2.0), p(2.0, 0.0, 2.0));
        let x0 = coedge_between(&a, top, p(0.0, 0.0, 2.0), p(0.0, 2.0, 2.0));
        let x2 = coedge_between(&a, top, p(2.0, 0.0, 2.0), p(2.0, 2.0, 2.0));
        Tab {
            a,
            b,
            top,
            y0,
            x0,
            x2,
            b_edges,
        }
    }

    /// The top with the footprint on it, the arcs `opposed` or not.
    fn run(&self, opposed: bool) -> Result<Realised, FaceError> {
        let a = &self.a;
        let (c00, c20) = ([0.0, 0.0, 2.0], [2.0, 0.0, 2.0]);
        // Records 0 and 1: `B`'s corners at `(0, 1)` and `(2, 1)`, on the
        // top's edges.
        let records = [
            RecPlace::On {
                line: LineRef::Coedge(self.x0),
                s: mid(a, self.x0),
            },
            RecPlace::On {
                line: LineRef::Coedge(self.x2),
                s: mid(a, self.x2),
            },
        ];
        let (r01, r21) = (VertRef::Rec(0), VertRef::Rec(1));
        let src = |k: usize| [(Operand::B, self.b_edges[k])];
        let (s0, s1, s2, s3) = (src(0), src(1), src(2), src(3));
        let along = |c, forward| {
            [AlongRef {
                line: LineRef::Coedge(c),
                forward,
            }]
        };
        // The footprint's loop `(0,0) → (2,0) → (2,1) → (0,1)`, with `B`
        // left of it where the top's own edges have the top left.
        let left = |c: CoedgeId, from: [f64; 3]| {
            let fwd = starts_at(a, c, from);
            let line = got_wind(a, self.top, c);
            // The arc's step: the top's line winds `line` along its
            // p-curve, so `B` inside the top winds the same way.
            (fwd, if fwd { line } else { -line })
        };
        // Each arc runs its line's way where the line starts where the
        // arc does, or past it.
        let (f_y0, w_y0) = left(self.y0, c00);
        let (f_x2, w_x2) = left(self.x2, c20);
        let (f_x0, w_x0) = left(self.x0, [0.0, 2.0, 2.0]);
        let a_y0 = along(self.y0, f_y0);
        let a_x2 = along(self.x2, f_x2);
        let a_x0 = along(self.x0, f_x0);
        let arcs = [
            ArcRec {
                ends: [end_at(a, self.y0, c00), end_at(a, self.y0, c20)],
                along: &a_y0,
                wind: w_y0,
                sources: &s0,
                opposed,
            },
            ArcRec {
                ends: [end_at(a, self.x2, c20), r21],
                along: &a_x2,
                wind: w_x2,
                sources: &s2,
                opposed,
            },
            // Transversal, `y = 1`: winds as the loop does.
            ArcRec {
                ends: [r21, r01],
                along: &[],
                wind: w_y0,
                sources: &s3,
                opposed,
            },
            ArcRec {
                ends: [r01, end_at(a, self.x0, c00)],
                along: &a_x0,
                wind: w_x0,
                sources: &s1,
                opposed,
            },
        ];
        let input = FaceArcs {
            operand: Operand::A,
            face: self.top,
            records: &records,
            lines: &[],
            arcs: &arcs,
        };
        run([&self.a, &self.b], &input)
    }

    /// The bundle holding the piece of `arc`.
    fn arc_bundle(got: &Realised, arc: u32) -> &Bundled {
        let p = got
            .coincident
            .pieces
            .iter()
            .find(|p| matches!(p.of, PieceOf::Arc { arc: a, .. } if a == arc))
            .unwrap();
        &got.bundles[p.bundle as usize]
    }
}

/// The winding step of `c`'s line on face `f` along its p-curve: `+1`
/// with the face to its left. Read from a run of the face alone.
fn got_wind(b: &Brep, f: FaceId, c: CoedgeId) -> i32 {
    let input = FaceArcs {
        operand: Operand::A,
        face: f,
        records: &[],
        lines: &[],
        arcs: &[],
    };
    let got = run([b, b], &input).unwrap();
    let l = got
        .lines
        .iter()
        .position(|&l| l == LineRef::Coedge(c))
        .unwrap();
    let piece = got
        .coincident
        .pieces
        .iter()
        .find(|p| {
            p.of == PieceOf::Line {
                line: l as u32,
                part: 0,
            }
        })
        .unwrap();
    got.coincident.bundles[piece.bundle as usize].wind
}

#[test]
fn a_tab_across_an_opposed_coincidence_joins_its_edges() {
    let tab = Tab::new(false);
    let got = tab.run(true).unwrap();
    // The footprint winds 2 (the top and `B`), the rest of the top 1.
    let mut windings: Vec<i64> = got
        .coincident
        .arrangement
        .regions()
        .iter()
        .map(|r| r.winding)
        .collect();
    windings.sort_unstable();
    assert_eq!(windings, [0, 1, 2]);
    let a_edge = |c| (Operand::A, tab.a.edge(c));
    let b_edge = |k: usize| (Operand::B, tab.b_edges[k]);
    // Along the top's edges: identified, joined across the coincidence,
    // each with both operands' edges.
    for (arc, c, k) in [(0, tab.y0, 0), (1, tab.x2, 2), (3, tab.x0, 1)] {
        let bd = Tab::arc_bundle(&got, arc);
        let mut want = vec![a_edge(c), b_edge(k)];
        want.sort_unstable();
        assert_eq!(bd.sources, want, "arc {arc}");
        assert!(bd.identified && bd.opposed_join && !bd.touch, "arc {arc}");
    }
    // Across the top: a new edge of its own.
    let bd = Tab::arc_bundle(&got, 2);
    assert_eq!(bd.sources, [b_edge(3)]);
    assert!(!bd.identified && !bd.opposed_join);
    // The halves of `x = 0` and `x = 2` past the records: the top's own.
    let lone: Vec<&Bundled> = got.bundles.iter().filter(|b| !b.identified).collect();
    assert_eq!(
        lone.len(),
        4,
        "the top's y = 2 edge, two halves, the cross arc"
    );
    // The arc along `x = 0` reuses its p-curve over the lower half.
    let x0 = got
        .lines
        .iter()
        .position(|&l| l == LineRef::Coedge(tab.x0))
        .unwrap() as u32;
    let p = got
        .coincident
        .pieces
        .iter()
        .find(|p| matches!(p.of, PieceOf::Arc { arc: 3, .. }))
        .unwrap();
    let (line, [s, e]) = p.reuses.unwrap();
    assert_eq!(line, x0);
    let m = mid(&tab.a, tab.x0);
    assert!(s == m || e == m);
    // The records are vertices of the realisation, on the top's edges.
    assert_eq!(got.verts[4..6], [VertOf::Rec(0), VertOf::Rec(1)]);
}

#[test]
fn the_join_needs_the_opposed_record() {
    let tab = Tab::new(false);
    let got = tab.run(false).unwrap();
    assert!(got.bundles.iter().all(|b| !b.opposed_join));
    assert_eq!(got.bundles.iter().filter(|b| b.identified).count(), 3);
}

#[test]
fn touch_is_the_or_over_sources() {
    let tab = Tab::new(true);
    let got = tab.run(true).unwrap();
    assert!(Tab::arc_bundle(&got, 0).touch);
    for arc in [1, 2, 3] {
        assert!(!Tab::arc_bundle(&got, arc).touch, "arc {arc}");
    }
}

#[test]
fn an_apex_is_two_face_vertices() {
    let cone = built(&Primitive::Cone {
        base: [0.0; 3],
        radii: [2.0, 0.0],
        height: 4.0,
    });
    // A side: a face with the apex's singular edge, whose p-curve is a
    // row of the face's UV, so the apex is seen at both its ends.
    let side = (0..cone.face_count() as u32)
        .map(FaceId)
        .find(|&f| {
            cone.loops(f)
                .flat_map(|l| cone.loop_coedges(l))
                .any(|c| cone.edge_kind(cone.edge(c)) == EdgeKind::Singular)
        })
        .unwrap();
    let input = FaceArcs {
        operand: Operand::A,
        face: side,
        records: &[],
        lines: &[],
        arcs: &[],
    };
    let got = run([&cone, &cone], &input).unwrap();
    let mut seen: BTreeMap<VertId, Vec<u32>> = BTreeMap::new();
    for v in &got.verts {
        if let VertOf::Face { vert, at } = *v {
            seen.entry(vert).or_default().push(at);
        }
    }
    let apex: Vec<_> = seen.values().filter(|ats| ats.len() == 2).collect();
    assert_eq!(apex.len(), 1, "{seen:?}");
    let [p, q] = [0, 1].map(|k| got.face_uvs[apex[0][k] as usize]);
    assert_ne!(p, q);
    assert_eq!(got.face_uvs.len(), got.coincident.points.len());
}

#[test]
fn refusals_name_their_entity() {
    let tab = Tab::new(false);
    let a = &tab.a;
    let bottom = face_at_z(a, 0.0);
    let other = a
        .loops(bottom)
        .flat_map(|l| a.loop_coedges(l))
        .next()
        .unwrap();
    let face = |records: &[RecPlace], arcs: &[ArcRec<'_>]| {
        let input = FaceArcs {
            operand: Operand::A,
            face: tab.top,
            records,
            lines: &[],
            arcs,
        };
        run([&tab.a, &tab.b], &input).map(|_| ())
    };
    let bad = VertRef::Face {
        coedge: other,
        end: false,
    };
    assert_eq!(face(&[RecPlace::At(bad)], &[]), Err(FaceError::Rec(0)));
    assert_eq!(
        face(
            &[RecPlace::On {
                line: LineRef::New(0),
                s: 0.5
            }],
            &[]
        ),
        Err(FaceError::Rec(0))
    );
    let [t0, _] = a.t_range(a.edge(tab.x0));
    assert_eq!(
        face(
            &[RecPlace::On {
                line: LineRef::Coedge(tab.x0),
                s: t0
            }],
            &[]
        ),
        Err(FaceError::Rec(0))
    );
    let corner = end_at(a, tab.y0, [0.0, 0.0, 2.0]);
    let far = end_at(a, tab.y0, [2.0, 0.0, 2.0]);
    let arc = |ends, sources: &'static [OperandEdge]| ArcRec {
        ends,
        along: &[],
        wind: 0,
        sources,
        opposed: false,
    };
    assert_eq!(
        face(&[], &[arc([corner, bad], &[])]),
        Err(FaceError::Arc(0))
    );
    assert_eq!(
        face(&[], &[arc([corner, far], &[(Operand::B, EdgeId(999))])]),
        Err(FaceError::Arc(0))
    );
    assert_eq!(
        face(&[], &[arc([corner, corner], &[])]),
        Err(FaceError::Arc(0))
    );
    let input = FaceArcs {
        operand: Operand::A,
        face: FaceId(99),
        records: &[],
        lines: &[],
        arcs: &[],
    };
    assert_eq!(run([a, a], &input).map(|_| ()), Err(FaceError::Face));
}

#[test]
fn small_budget_runs_out() {
    let tab = Tab::new(false);
    let input = FaceArcs {
        operand: Operand::A,
        face: tab.top,
        records: &[],
        lines: &[],
        arcs: &[],
    };
    let got = realise_on([&tab.a, &tab.b], &input, &mut Work::new(&Budget::new(2)));
    assert!(matches!(got, Err(FaceError::OutOfBudget(_))), "{got:?}");
}
