use super::*;
use crate::rng::Rng;
use crate::sheets::{
    Diagonals, OFFSET, Wavy, box_edge_fold, l_corner, l_corner_quad, smooth, wavy,
};
use malachite::base::num::arithmetic::traits::PowerOf2;
use malachite::base::num::basic::traits::One;
use malachite::base::num::conversion::traits::RoundingFrom;
use malachite::base::rounding_modes::RoundingMode;

fn r(x: f64) -> Rational {
    q(x)
}

fn p3(x: [f64; 3]) -> P3 {
    x.map(q)
}

fn sheet(verts: Vec<[f64; 3]>, faces: Vec<Vec<u32>>) -> Polygons {
    Polygons {
        closed: false,
        verts,
        faces,
    }
}

fn find(pairs: &[FacePair], a: u32, b: u32) -> &FacePair {
    pairs
        .iter()
        .find(|p| p.a == a && p.b == b)
        .unwrap_or_else(|| panic!("no pair ({a}, {b}) in {pairs:?}"))
}

/// The box-edge fold's sheet lies its offset outside the box's top and
/// front faces, exactly as far as its stored coordinates say, and
/// meets nothing.
#[test]
fn box_edge_fold_distances_are_the_stored_offsets() {
    let pair = box_edge_fold(OFFSET);
    let ex = ExactPair::new(&pair).unwrap();
    let dz = r(pair.b.verts[0][2]) - Rational::from(20);
    let dy = -r(pair.b.verts[0][1]);
    assert!(dz > 0 && dy > 0);
    let pairs = ex.face_pairs(2.0 * OFFSET).unwrap();
    // The prism's faces: bottom, top, then a side per base edge (the
    // first is the front, `y = 0`); the sheet's: top quad, front quad.
    assert_eq!(find(&pairs, 1, 0).dist2, &dz * &dz);
    assert_eq!(find(&pairs, 2, 1).dist2, &dy * &dy);
    // Across the fold: the top quad over the front face's top edge, the
    // front quad beside the top face's front edge.
    assert_eq!(find(&pairs, 2, 0).dist2, &dz * &dz);
    assert_eq!(find(&pairs, 1, 1).dist2, &dy * &dy);
    assert!(pairs.iter().all(|p| p.kind == Kind::Apart && p.dist2 > 0));
}

/// Flush, the fold's quads lie in the box's top and front planes facing
/// the same way, overlapping in their whole area, and each meets the
/// other face along the box edge without crossing it.
#[test]
fn flush_box_edge_fold_is_coplanar_and_touches_along_the_edge() {
    let ex = ExactPair::new(&box_edge_fold(0.0)).unwrap();
    let pairs = ex.face_pairs(0.0).unwrap();
    for (a, b) in [(1, 0), (2, 1)] {
        let Kind::Coplanar {
            opposed,
            pieces,
            area2,
        } = &find(&pairs, a, b).kind
        else {
            panic!("({a}, {b}) not coplanar: {pairs:?}");
        };
        assert!(!opposed);
        assert!(!pieces.is_empty());
        assert_eq!(*area2, Rational::from(300 * 300));
    }
    for (a, b) in [(2, 0), (1, 1)] {
        let p = find(&pairs, a, b);
        let Kind::Meet {
            segments,
            a_crosses,
            b_crosses,
        } = &p.kind
        else {
            panic!("({a}, {b}) not meeting: {p:?}");
        };
        assert!(!a_crosses && !b_crosses && !p.is_transversal());
        let mut ends = segments[0].clone();
        ends.sort();
        assert_eq!(segments.len(), 1);
        assert_eq!(ends, [p3([10.0, 0.0, 20.0]), p3([30.0, 0.0, 20.0])]);
    }
}

/// Over a wavy pair, `b` lies its offset above `a` along `z` everywhere
/// (to the rounding of its heights), the cells cover `a`'s square
/// exactly, and the sheets meet nowhere.
#[test]
fn wavy_gap_is_the_offset_over_the_whole_sheet() {
    let w = Wavy::DEFAULT;
    let ex = ExactPair::new(&wavy(&w)).unwrap();
    let all = |n: usize| (0..n as u32).collect::<Vec<_>>();
    let g = ex
        .gap(
            &all(ex.a.faces.len()),
            &all(ex.b.faces.len()),
            [0.0, 0.0, 1.0],
        )
        .unwrap();
    assert_eq!(g.axis, 2);
    assert_eq!(g.area(), Rational::from(6400));
    let [lo, hi] = g.range().unwrap();
    let off = r(w.offset);
    let slack = r(1e-12);
    assert!(lo > &off - &slack && hi < &off + &slack, "{lo} {hi}");
    assert!(g.cells.iter().all(|c| c.zero_set() == ZeroSet::None));
    let pairs = ex.face_pairs(w.offset).unwrap();
    assert!(!pairs.is_empty());
    assert!(pairs.iter().all(|p| p.kind == Kind::Apart));
}

/// Flush wavy sheets cut alike coincide: each face of `b` lies in its
/// twin's plane facing the same way, and the gap is zero on every cell.
#[test]
fn flush_wavy_sheets_coincide_face_by_face() {
    let w = Wavy {
        cells: 4,
        offset: 0.0,
        diagonals_b: Diagonals::Rising,
        ..Wavy::DEFAULT
    };
    let ex = ExactPair::new(&wavy(&w)).unwrap();
    let pairs = ex.face_pairs(0.0).unwrap();
    for f in 0..ex.b.faces.len() as u32 {
        let p = find(&pairs, f, f);
        assert!(
            matches!(&p.kind, Kind::Coplanar { opposed: false, area2, .. } if *area2 > 0),
            "{p:?}"
        );
    }
    let all: Vec<u32> = (0..ex.a.faces.len() as u32).collect();
    let g = ex.gap(&all, &all, [0.0, 0.0, 1.0]).unwrap();
    assert_eq!(g.area(), Rational::from(6400));
    assert!(g.cells.iter().all(|c| c.zero_set() == ZeroSet::Whole));
}

/// The L-corner quad lies its offset above the L's corner square, and
/// seen along `z` its two outer sides run along the L's outer edges:
/// the two zero-area regions a snap leaves there.
#[test]
fn l_corner_quad_sides_run_along_the_outer_edges() {
    let pair = l_corner_quad(OFFSET);
    let ex = ExactPair::new(&pair).unwrap();
    let dz = r(pair.b.verts[0][2]);
    let g = ex.gap(&[0], &[0], [0.0, 0.0, 1.0]).unwrap();
    assert_eq!(g.area(), Rational::from(225));
    assert_eq!(g.range().unwrap(), [dz.clone(), dz.clone()]);
    let edges = ex.coincident_edges([0.0, 0.0, 1.0]).unwrap();
    let got: Vec<([u32; 2], [u32; 2])> = edges.iter().map(|e| (e.a, e.b)).collect();
    // The L's (0,0)–(40,0) and (0,40)–(0,0); the quad's sides on them.
    assert_eq!(got, vec![([0, 1], [0, 1]), ([0, 5], [0, 3])]);
    for e in &edges {
        assert_eq!(e.s, [dz.clone(), dz.clone()]);
        let len2 = dist2(&e.on_a[0], &e.on_a[1]);
        assert_eq!(len2, Rational::from(225));
    }
    // Flush, the quad lies in the L's plane over its corner square.
    let flush = ExactPair::new(&l_corner_quad(0.0)).unwrap();
    let pairs = flush.face_pairs(0.0).unwrap();
    assert!(matches!(
        &find(&pairs, 0, 0).kind,
        Kind::Coplanar { opposed: false, area2, .. } if *area2 == 225 * 225
    ));
    let g = flush.gap(&[0], &[0], [0.0, 0.0, 1.0]).unwrap();
    assert!(g.cells.iter().all(|c| c.zero_set() == ZeroSet::Whole));
}

/// The L-shaped prism's winding number: one inside, zero in its notch
/// and beyond, on its faces and its reflex edge the boundary; the folded
/// sheet lies outside.
#[test]
fn l_prism_winding_numbers() {
    let pair = l_corner(OFFSET);
    let ex = ExactPair::new(&pair).unwrap();
    let w = |p: [f64; 3]| ex.a.winding(&p3(p)).unwrap();
    assert_eq!(w([5.0, 5.0, 10.0]), Winding::Number(1));
    assert_eq!(w([35.0, 10.0, 1.0]), Winding::Number(1));
    assert_eq!(w([30.0, 30.0, 10.0]), Winding::Number(0));
    assert_eq!(w([50.0, 5.0, 10.0]), Winding::Number(0));
    assert_eq!(w([5.0, 5.0, 25.0]), Winding::Number(0));
    assert_eq!(w([15.0, 20.0, 10.0]), Winding::On);
    assert_eq!(w([15.0, 15.0, 10.0]), Winding::On);
    assert_eq!(w([0.0, 0.0, 0.0]), Winding::On);
    for v in &pair.b.verts {
        assert_eq!(w(*v), Winding::Number(0), "{v:?}");
    }
    assert_eq!(
        ex.b.winding(&p3([0.0; 3])),
        Err(OracleError::NotClosed { side: Side::B })
    );
}

/// A geometric digest of a pair's answers: no face or vertex ids.
fn digest(ex: &ExactPair, within: f64, ell: [f64; 3]) -> Vec<String> {
    let mut out: Vec<String> = ex
        .face_pairs(within)
        .unwrap()
        .iter()
        .map(|p| {
            let kind = match &p.kind {
                Kind::Apart => "apart".to_string(),
                Kind::Coplanar { opposed, area2, .. } => format!("coplanar {opposed} {area2}"),
                Kind::Meet {
                    segments,
                    a_crosses,
                    b_crosses,
                } => format!("meet {} {a_crosses} {b_crosses}", segments.len()),
            };
            format!("{} {kind}", p.dist2)
        })
        .collect();
    let a = ex.a.facing(ell).unwrap();
    let b = ex.b.facing(ell).unwrap();
    let g = ex.gap(&a, &b, ell).unwrap();
    out.push(format!("gap {:?} {}", g.range(), g.area()));
    let mut edges: Vec<String> = ex
        .coincident_edges(ell)
        .unwrap()
        .iter()
        .map(|e| {
            format!(
                "edge {} {} {}",
                dist2(&e.on_a[0], &e.on_a[1]),
                e.s[0],
                e.s[1]
            )
        })
        .collect();
    out.append(&mut edges);
    out.sort();
    out
}

/// Renumbering the vertices and faces (and turning the loops) changes
/// none of the oracle's answers.
#[test]
fn answers_follow_the_geometry_not_the_ids() {
    let mut rng = Rng::new(17);
    let inset = Wavy {
        cells: 4,
        inset: 1,
        diagonals_b: Diagonals::Alternating,
        ..Wavy::DEFAULT
    };
    let cases = [
        (box_edge_fold(OFFSET), [0.0, 0.0, 1.0]),
        (box_edge_fold(0.0), [0.0, -1.0, 0.0]),
        (l_corner_quad(OFFSET), [0.0, 0.0, 1.0]),
        (wavy(&inset), [0.0, 0.0, 1.0]),
    ];
    for (pair, ell) in cases {
        let ex = ExactPair::new(&pair).unwrap();
        let moved = Pair {
            a: pair.a.relabelled(&mut rng),
            b: pair.b.relabelled(&mut rng),
        };
        let ey = ExactPair::new(&moved).unwrap();
        assert_eq!(
            digest(&ex, 2.0 * OFFSET, ell),
            digest(&ey, 2.0 * OFFSET, ell)
        );
    }
}

/// An exact quarter turn about `z` of both bodies (and of `ℓ`) changes
/// no distance, gap or area.
#[test]
fn quarter_turns_change_nothing() {
    let w = Wavy {
        cells: 4,
        inset: 1,
        diagonals_b: Diagonals::Alternating,
        ..Wavy::DEFAULT
    };
    let turn = |p: &Polygons| Polygons {
        verts: p.verts.iter().map(|v| [-v[1], v[0], v[2]]).collect(),
        ..p.clone()
    };
    for (pair, ell, turned) in [
        (wavy(&w), [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]),
        (box_edge_fold(0.0), [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
    ] {
        let ex = ExactPair::new(&pair).unwrap();
        let ey = ExactPair::new(&Pair {
            a: turn(&pair.a),
            b: turn(&pair.b),
        })
        .unwrap();
        assert_eq!(
            digest(&ex, 2.0 * OFFSET, ell),
            digest(&ey, 2.0 * OFFSET, turned)
        );
    }
}

/// A tilted sheet through a flat one: the faces cross along their
/// common line, and the gap along `z` changes sign there, its zero set
/// that line over the whole square.
#[test]
fn a_tilted_sheet_crosses_along_the_gap_s_zero_line() {
    let a = sheet(
        vec![
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [0.0, 4.0, 0.0],
        ],
        vec![vec![0, 1, 2, 3]],
    );
    let b = sheet(
        vec![
            [0.0, 0.0, -0.25],
            [4.0, 0.0, 0.25],
            [4.0, 4.0, 0.25],
            [0.0, 4.0, -0.25],
        ],
        vec![vec![0, 1, 2, 3]],
    );
    let ex = ExactPair::new(&Pair { a, b }).unwrap();
    let pairs = ex.face_pairs(0.0).unwrap();
    assert!(pairs[0].is_transversal());
    let Kind::Meet { segments, .. } = &pairs[0].kind else {
        panic!("{pairs:?}");
    };
    let mut s = segments[0].clone();
    s.sort();
    assert_eq!(s, [p3([2.0, 0.0, 0.0]), p3([2.0, 4.0, 0.0])]);
    let g = ex.gap(&[0], &[0], [0.0, 0.0, 1.0]).unwrap();
    assert_eq!(g.range().unwrap(), [r(-0.25), r(0.25)]);
    assert_eq!(g.area(), Rational::from(16));
    // The zero line, as the union of the cells' zero segments over `y`.
    let mut ys: Vec<[Rational; 2]> = Vec::new();
    for c in &g.cells {
        match c.zero_set() {
            ZeroSet::Segment([p, q]) => {
                assert!(p[0] == 2 && q[0] == 2 && p[2] == 0 && q[2] == 0);
                let (lo, hi) = if p[1] < q[1] {
                    (p[1].clone(), q[1].clone())
                } else {
                    (q[1].clone(), p[1].clone())
                };
                ys.push([lo, hi]);
            }
            ZeroSet::Point(p) => assert!(p[0] == 2),
            other => panic!("{other:?}"),
        }
    }
    ys.sort();
    assert_eq!(ys.first().map(|y| y[0].clone()), Some(Rational::ZERO));
    assert!(ys.windows(2).all(|w| w[1][0] <= w[0][1]));
    assert_eq!(
        ys.iter().map(|y| y[1].clone()).max(),
        Some(Rational::from(4))
    );
}

/// Two triangles whose closest points are inside an edge of each (skew
/// edges a unit apart, every corner farther): the distance is the
/// edges'.
#[test]
fn skew_edges_give_the_distance_between_their_interiors() {
    let a = sheet(
        vec![[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -2.0]],
        vec![vec![0, 1, 2]],
    );
    let b = sheet(
        vec![[0.0, -1.0, 1.0], [0.0, 1.0, 1.0], [0.0, 0.0, 3.0]],
        vec![vec![0, 1, 2]],
    );
    let ex = ExactPair::new(&Pair { a, b }).unwrap();
    let pairs = ex.face_pairs(2.0).unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].dist2, Rational::ONE);
    assert_eq!(pairs[0].kind, Kind::Apart);
}

/// Inputs the oracle can't take are named, not guessed at.
#[test]
fn bad_inputs_are_errors() {
    let bent = sheet(
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 1e-9],
            [0.0, 1.0, 0.0],
        ],
        vec![vec![0, 1, 2, 3]],
    );
    assert!(matches!(
        ExactBody::new(&bent, Side::A),
        Err(OracleError::NotPlanar { face: 0, .. })
    ));
    let line = sheet(
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        vec![vec![0, 1, 2]],
    );
    assert!(matches!(
        ExactBody::new(&line, Side::B),
        Err(OracleError::BadFace {
            side: Side::B,
            face: 0
        })
    ));
    let ex = ExactPair::new(&box_edge_fold(OFFSET)).unwrap();
    assert_eq!(
        ex.gap(&[2], &[0], [0.0, 0.0, 1.0]),
        Err(OracleError::NotGraph {
            side: Side::A,
            face: 2
        })
    );
    assert_eq!(ex.gap(&[1], &[0], [0.0; 3]), Err(OracleError::BadInput));
    assert_eq!(ex.face_pairs(-1.0), Err(OracleError::BadInput));
}

/// `D`'s Bernstein form is the squared distance to the axis, exactly, at
/// sample parameters.
#[test]
fn smooth_distance_form_is_exact() {
    let s = SmoothOracle::new(&smooth(1e-4)).unwrap();
    for (u, v) in [(0, 0), (1, 3), (5, 2), (7, 7), (3, 8)] {
        let (u, v) = (Rational::from_signeds(u, 7), Rational::from_signeds(v, 8));
        assert_eq!(s.d_at(&u, &v) - s.radius() * s.radius(), s.implicit(&u, &v));
    }
}

/// The clear pair's least gap is its stated gap, enclosed in big floats
/// to far below an f64's precision, and the comparison accepts a value
/// within its error of it and rejects one off by more.
#[test]
fn smooth_clear_pair_encloses_its_least_gap() {
    let gap = 1e-4;
    let s = SmoothOracle::new(&smooth(gap)).unwrap();
    let rel = s.relation(&Rational::power_of_2(-60i64), 4000);
    let SmoothRelation::Clear(least) = rel else {
        panic!("{rel:?}");
    };
    let b = s.gap_bounds(&least, 256);
    let [lo, hi] = b.f64_bounds();
    assert!(lo <= hi && hi - lo < 1e-15, "{lo} {hi}");
    assert!((lo - gap).abs() < 1e-12, "{lo}");
    assert!(b.meets(gap, 1e-12));
    assert!(!b.meets(gap + 1e-9, 1e-12));
    assert!(!b.meets(f64::NAN, 1.0));
}

/// The overlapping pair dips into the cylinder, and along `v = ½` its
/// gap is zero at two parameters, where `r + g − x²/(4r) = √(r² − x²)`:
/// `x ≈ ±√(−4·r·g)`.
#[test]
fn smooth_overlapping_pair_crosses_where_its_gap_is_zero() {
    let g = -1e-4;
    let s = SmoothOracle::new(&smooth(g)).unwrap();
    let rel = s.relation(&Rational::power_of_2(-40i64), 4000);
    let SmoothRelation::Crossing(least) = rel else {
        panic!("{rel:?}");
    };
    assert!(s.implicit(&least.at[0], &least.at[1]) < 0);
    let half = Rational::from_signeds(1i64, 2);
    let width = Rational::power_of_2(-40i64);
    let roots = s.roots(&Line::AlongU { v: half.clone() }, &width);
    assert_eq!(roots.len(), 2, "{roots:?}");
    let x = libm::sqrt(-4.0 * 50.0 * g);
    for (root, want) in roots.iter().zip([-x, x]) {
        assert!(root.single && &root.hi - &root.lo <= width);
        let lo = s.implicit(&root.lo, &half);
        let hi = s.implicit(&root.hi, &half);
        assert!(lo * hi < 0);
        let at = s.point(&root.lo, &half);
        let got = f64::rounding_from(&at[0], RoundingMode::Nearest).0;
        assert!((got - want).abs() < 1e-6, "{got} {want}");
    }
    // Along an edge of the sheet it clears the cylinder.
    assert!(
        s.roots(&Line::AlongV { u: Rational::ZERO }, &width)
            .is_empty()
    );
}
