//! Invariant 4 on planar bodies: the valid ones pass (a pinched bottom
//! as `fC ∪ lNeck` leaves it, a frame, a hole cut by a slit closed by
//! zero-length edges), and each defect seeded into one is caught by the
//! check that names it, at the face it is on.

use super::*;
use crate::geom::lattice::CELL;
use crate::infra::budget::{Budget, Work};
use crate::topo::brep::{BodyKind, Contact, ContactCert, LoopId};
use crate::topo::check::topo::tests::{
    Passing, Poly, PolyFace, build, cube, failure, frame, half_sphere, pinched, square,
};
use crate::topo::check::topo::{Domain, Joins, Topology};
use crate::topo::check::{CheckError, CheckFailure, Inv, Invariants, Scope, run};

/// Invariants 1 to 4, or 4 alone.
fn table(all: bool) -> Invariants<'static> {
    let p: &dyn Invariant = &Passing;
    if all {
        [&Topology, &Joins, &Domain, &Trims, p, p, p, p, p, p]
    } else {
        [p, p, p, &Trims, p, p, p, p, p, p]
    }
}

fn check_with(b: &Brep, all: bool) -> Result<(), CheckError> {
    let work = &mut Work::new(&Budget::DEFAULT);
    run(b, Scope::Full, None, &table(all), work, 0)
}

/// Invariants 1 to 4 (`all`) or 4 alone on `b`, the same on one thread
/// and on several.
fn check(b: &Brep, all: bool) -> Result<(), CheckError> {
    #[cfg(not(target_arch = "wasm32"))]
    return crate::infra::par::assert_deterministic(|| check_with(b, all));
    #[cfg(target_arch = "wasm32")]
    return check_with(b, all);
}

/// The failure of invariant 4 on `b` (invariants 1 to 3 passing first if
/// `all`): its check and face.
fn fails(b: &Brep, all: bool) -> (TrimCheck, Item) {
    let f: CheckFailure = failure(check(b, all));
    assert_eq!(f.inv, Inv::Trims, "{f:?}");
    let checks = [
        TrimCheck::Linear,
        TrimCheck::SpanFold,
        TrimCheck::OuterLoops,
        TrimCheck::ZeroLengthReturn,
        TrimCheck::Revisit,
        TrimCheck::PinchArea,
        TrimCheck::Crossing,
        TrimCheck::Contact,
        TrimCheck::Orientation,
        TrimCheck::Nesting,
    ];
    let c = checks[f.check as usize - 1];
    assert_eq!(c as u32, f.check);
    assert_eq!(f.finding, c.finding());
    (c, f.item)
}

/// A sheet of one face in the plane `z = 0`, its loops of vertex indices
/// at `pos` (`(x, y)`), the outer first.
fn sheet(pos: &[[f64; 2]], loops: &[&[usize]]) -> Brep {
    let p = Poly {
        pos: pos.iter().map(|q| [q[0], q[1], 0.0]).collect(),
        faces: vec![PolyFace {
            loops: loops.iter().map(|l| l.to_vec()).collect(),
            part: 0,
        }],
    };
    build(&p, BodyKind::Sheet)
}

/// The square `[0, 4]²`'s corners.
const OUTLINE: [[f64; 2]; 4] = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];

/// `b` with every p-curve of loop `l` moved by `d` in UV.
fn shift_loop(mut b: Brep, l: LoopId, d: [f64; 2]) -> Brep {
    for c in 0..b.coedge_count() {
        if b.coedges.lp[c] != l {
            continue;
        }
        let id = b.coedges.pcurve[c].index();
        let p = &b.pcurves[id];
        let spans = (p.q_spans().unwrap().into_iter())
            .map(|s| s.map(|q| [q[0] + d[0], q[1] + d[1], q[2]]))
            .collect();
        b.pcurves[id] = PCurve::new(spans, p.knots().to_vec()).unwrap();
    }
    b
}

/// The edge between vertices `a` and `z` (input indices).
fn edge(b: &Brep, a: usize, z: usize) -> EdgeId {
    let ends = [VertId(a.min(z) as u32), VertId(a.max(z) as u32)];
    (0..b.edge_count() as u32)
        .map(EdgeId)
        .find(|&e| b.ends(e) == ends)
        .expect("an edge")
}

fn contact(kind: ContactKind, members: Vec<[Member; 2]>) -> Contact {
    Contact {
        kind,
        members,
        cert: ContactCert::default(),
    }
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
        assert_eq!(check(&b, true), Ok(()), "{name}");
    }
}

/// The bottom `fC ∪ lNeck` leaves: one loop passing the L's reflex
/// corner twice, the square at the corner a lobe turning against the
/// rest, inside it. Split into an outer loop and a hole through that
/// vertex (which invariant 1 refuses), the loops meet at one vertex
/// without crossing.
#[test]
fn a_pinch_passes() {
    let one = build(&pinched(false), BodyKind::Solid);
    assert_eq!(check(&one, true), Ok(()));
    let split = build(&pinched(true), BodyKind::Solid);
    assert_eq!(check(&split, false), Ok(()));
}

/// A pinch whose lobes lie apart turning one way passes; one lobe
/// turning the other way, or a lobe inside the other turning the same
/// way, crosses.
#[test]
fn a_pinch_lobes_turn_as_they_nest() {
    let pos = [
        [0.0, 0.0],
        [2.0, 0.0],
        [2.0, 2.0],
        [4.0, 2.0],
        [4.0, 4.0],
        [2.0, 4.0],
        [3.0, 3.0],
        [3.5, 2.5],
    ];
    // A triangle and a square touching at their corners `(2, 2)`.
    let apart = sheet(&pos, &[&[0, 1, 2, 3, 4, 5, 2]]);
    assert_eq!(check(&apart, false), Ok(()));
    let against = sheet(&pos, &[&[1, 0, 2, 3, 4, 5, 2]]);
    assert_eq!(fails(&against, false).0, TrimCheck::Crossing);
    // A triangle inside the square, at its corner, the same way round.
    let nested = sheet(&pos, &[&[2, 3, 4, 5, 2, 7, 6]]);
    assert_eq!(fails(&nested, false).0, TrimCheck::Crossing);
    // Turning the other way it is a hole touching the outline.
    let keyhole = sheet(&pos, &[&[2, 3, 4, 5, 2, 6, 7]]);
    assert_eq!(check(&keyhole, false), Ok(()));
}

/// A loop passing a vertex twice with zero area on one side (it runs out
/// along an edge and back) is caught, as is a loop passing a point
/// three times.
#[test]
fn a_pinch_needs_area_on_both_sides() {
    let mut pos = OUTLINE.to_vec();
    pos.push([2.0, 2.0]);
    let b = sheet(&pos, &[&[0, 1, 2, 4, 2, 3]]);
    assert_eq!(
        fails(&b, true),
        (TrimCheck::PinchArea, Item::Face(FaceId(0)))
    );
    // Three visits of the corner `(4, 4)`.
    let pos = [
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 4.0],
        [6.0, 4.0],
        [6.0, 6.0],
        [4.0, 8.0],
        [2.0, 8.0],
        [2.0, 6.0],
    ];
    let b = sheet(&pos, &[&[0, 1, 2, 3, 4, 2, 5, 6, 2, 7]]);
    assert_eq!(fails(&b, false).0, TrimCheck::Revisit);
}

/// A loop returning to a vertex through a run of zero-length edges is
/// caught.
#[test]
fn a_return_through_zero_length_edges_is_caught() {
    let mut pos = OUTLINE.to_vec();
    pos.push(OUTLINE[2]);
    // Out to the corner's twin and back along one zero-length edge.
    let b = sheet(&pos, &[&[0, 1, 2, 4, 2, 3]]);
    assert_eq!(
        fails(&b, true),
        (TrimCheck::ZeroLengthReturn, Item::Face(FaceId(0)))
    );
    // Through two twins, as `cube ∪ lSlit` turned can leave `7, 37, 36, 7`.
    pos.push(OUTLINE[2]);
    let b = sheet(&pos, &[&[0, 1, 2, 4, 5, 2, 3]]);
    assert_eq!(fails(&b, false).0, TrimCheck::ZeroLengthReturn);
    // A run through the twins without returning is one corner.
    let b = sheet(&pos, &[&[0, 1, 2, 4, 5, 3]]);
    assert_eq!(check(&b, false), Ok(()));
}

/// A hole cut by a slit: its two sides on two edges sharing one p-curve,
/// closed at its ends by zero-length edges, a zero-area hole. It passes
/// with its edges in an edge contact, and crosses itself without.
#[test]
fn a_slit_passes_with_its_contact() {
    let mut pos = OUTLINE.to_vec();
    pos.extend([[1.0, 2.0], [3.0, 2.0], [1.0, 2.0], [3.0, 2.0]]);
    // Out along `4 → 5`, over to `7`, back along `7 → 6` (the edge
    // `6 → 7` run backwards: the same p-curve), over to `4`.
    let mut b = sheet(&pos, &[&[0, 1, 2, 3], &[4, 5, 7, 6]]);
    assert_eq!(
        fails(&b, true),
        (TrimCheck::Crossing, Item::Face(FaceId(0)))
    );
    let sides = [Member::Edge(edge(&b, 4, 5)), Member::Edge(edge(&b, 6, 7))];
    b.contacts.push(contact(ContactKind::Edge, vec![sides]));
    assert_eq!(check(&b, true), Ok(()));
    // The slit taken for the outer loop, the square for a hole: the slit
    // bounds no area.
    b.loops.is_outer.swap(0, 1);
    assert_eq!(fails(&b, true).0, TrimCheck::Orientation);
}

/// Crossing trims are caught: a hole moved over the outer loop's side,
/// and a loop crossing itself.
#[test]
fn crossing_trims_are_caught() {
    let b = build(&frame(), BodyKind::Sheet);
    let b = shift_loop(b, LoopId(1), [1.5 / CELL, 0.0]);
    assert_eq!(
        fails(&b, true),
        (TrimCheck::Crossing, Item::Face(FaceId(0)))
    );
    let pos = [[0.0, 0.0], [3.0, 2.0], [3.0, 0.0], [0.0, 1.0]];
    let b = sheet(&pos, &[&[0, 1, 2, 3]]);
    assert_eq!(fails(&b, true).0, TrimCheck::Crossing);
    // A hole touching the outer loop's side at its vertex.
    let mut pos = OUTLINE.to_vec();
    pos.extend([[2.0, 0.0], [3.0, 2.0], [1.0, 2.0]]);
    let b = sheet(&pos, &[&[0, 1, 2, 3], &[4, 6, 5]]);
    assert_eq!(fails(&b, true).0, TrimCheck::Crossing);
}

/// Two loops meeting at a point must meet at one contact point there:
/// two vertices apart are caught, two in a vertex contact pass.
#[test]
fn loops_meet_at_a_contact_point() {
    let mut pos = OUTLINE.to_vec();
    pos.extend([[0.0, 0.0], [1.0, 2.0], [2.0, 1.0]]);
    let mut b = sheet(&pos, &[&[0, 1, 2, 3], &[4, 5, 6]]);
    assert_eq!(fails(&b, true), (TrimCheck::Contact, Item::Face(FaceId(0))));
    let point = [Member::Vert(VertId(0)), Member::Vert(VertId(4))];
    b.contacts.push(contact(ContactKind::Vertex, vec![point]));
    assert_eq!(check(&b, true), Ok(()));
}

/// Each remaining defect seeded into a frame or a square, caught by its
/// check at its face.
#[test]
fn trim_defects_are_caught() {
    use TrimCheck as T;
    let frame = build(&frame(), BodyKind::Sheet);
    let at = (T::Nesting, Item::Face(FaceId(0)));
    // The hole moved out of the outer loop.
    let out = shift_loop(frame.clone(), LoopId(1), [3.0 / CELL, 0.0]);
    assert_eq!(fails(&out, true), at);
    // A hole inside another.
    let mut pos: Vec<[f64; 2]> = OUTLINE.iter().map(|p| p.map(|x| 2.0 * x)).collect();
    pos.extend(OUTLINE.iter().map(|p| p.map(|x| 1.0 + 1.5 * x)));
    pos.extend(OUTLINE.iter().map(|p| p.map(|x| 3.0 + 0.5 * x)));
    let b = sheet(&pos, &[&[0, 1, 2, 3], &[4, 7, 6, 5], &[8, 11, 10, 9]]);
    assert_eq!(fails(&b, true), at);
    let b = sheet(&pos, &[&[0, 1, 2, 3], &[8, 11, 10, 9], &[4, 7, 6, 5]]);
    assert_eq!(fails(&b, true), at);
    let b = sheet(&pos, &[&[0, 1, 2, 3], &[4, 7, 6, 5]]);
    assert_eq!(check(&b, false), Ok(()));
    // The face turned over: its loops turn the wrong way.
    let mut b = frame.clone();
    b.faces.flipped[0] ^= true;
    assert_eq!(fails(&b, true), (T::Orientation, Item::Face(FaceId(0))));
    // The hole marked outer; no loop outer.
    let mut b = frame.clone();
    b.loops.is_outer[1] = true;
    assert_eq!(fails(&b, true).0, T::OuterLoops);
    let mut b = frame.clone();
    b.loops.is_outer[0] = false;
    assert_eq!(fails(&b, true).0, T::OuterLoops);
    // A curved trim: the face needs the curved part, not built.
    let mut b = build(&square(), BodyKind::Sheet);
    let id = b.coedges.pcurve[0].index();
    b.pcurves[id] = b.pcurves[id].perturbed(&[[1.0 / CELL; 2]]).unwrap();
    let f = b.coedges.face[0];
    assert_eq!(fails(&b, true), (T::Linear, Item::Face(f)));
    // A Q span on its line running past its end and back.
    let mut b = build(&square(), BodyKind::Sheet);
    let mut spans = b.pcurves[id].q_spans().unwrap();
    let [a, _, z] = spans[0];
    spans[0][1] = [z[0] + (z[0] - a[0]), z[1] + (z[1] - a[1]), 1.0];
    b.pcurves[id] = PCurve::new(spans, b.pcurves[id].knots().to_vec()).unwrap();
    assert_eq!(fails(&b, true), (T::SpanFold, Item::Face(f)));
}
