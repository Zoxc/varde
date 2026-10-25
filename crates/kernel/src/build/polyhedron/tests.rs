//! Bodies from meshes of the shapes other kernels' flat test corpora
//! hold (a cube, a tetrahedron, an octahedron, a Menger sponge, a prism
//! on a rounded polygon, a staircase, a subdivided icosahedron): their
//! faces merged by plane, their check, their exact volumes; and the
//! meshes refused.

use malachite::base::num::basic::traits::Zero;

use super::*;
use crate::infra::ops::{FeatureId, InstanceId};
use crate::measure::mass::mass_props;
use crate::topo::adjacent::EmbedCheck;
use crate::topo::check::{CheckError, Finding, Inv};
use varde_decide::num::storage::nearest;
use varde_decide::num::trig;

fn ctx() -> OpCtx {
    OpCtx {
        feature: FeatureId(5),
        instance: InstanceId(1),
    }
}

/// A mesh: vertices and counter-clockwise triangles.
#[derive(Clone, Debug, Default)]
struct Tris {
    verts: Vec<[f64; 3]>,
    tris: Vec<[usize; 3]>,
}

impl Tris {
    /// The body, unchecked, with the work it took.
    fn build(&self) -> Result<(Brep, u64), PolyhedronError> {
        let mut work = Work::new(&Budget::DEFAULT);
        let b = build(
            &self.verts,
            &self.tris,
            ctx(),
            Tolerance::DEFAULT,
            &mut work,
        )?;
        Ok((b, work.used()))
    }

    fn body(&self) -> Brep {
        self.build().unwrap_or_else(|e| panic!("{e:?}")).0
    }

    /// The exact volume: the triangles' signed tetrahedra from the
    /// origin.
    fn volume(&self) -> Rational {
        let mut v = Rational::ZERO;
        for t in &self.tris {
            let [a, b, c] = t.map(|i| self.verts[i].map(q));
            v += dot(&a, &cross(&b, &c));
        }
        v / Rational::from(6u32)
    }

    /// The mesh with each quad `[a, b, c, d]` (counter-clockwise) as two
    /// triangles split along `a–c`.
    fn quads(verts: Vec<[f64; 3]>, quads: &[[usize; 4]]) -> Tris {
        let tris = quads
            .iter()
            .flat_map(|&[a, b, c, d]| [[a, b, c], [a, c, d]])
            .collect();
        Tris { verts, tris }
    }
}

/// The closed surface of a set of unit cells (filled where `cell` says)
/// of the `n³` grid scaled by `s`: two triangles for each side between a
/// filled cell and an empty one (or the outside).
fn cells(n: usize, s: f64, cell: impl Fn([usize; 3]) -> bool) -> Tris {
    let filled =
        |c: [i64; 3]| c.iter().all(|&x| (0..n as i64).contains(&x)) && cell(c.map(|x| x as usize));
    let mut index = BTreeMap::new();
    let mut verts = Vec::new();
    let mut vert = |p: [i64; 3]| -> usize {
        *index.entry(p).or_insert_with(|| {
            verts.push(p.map(|x| x as f64 * s));
            verts.len() - 1
        })
    };
    let mut quads = Vec::new();
    for x in 0..n as i64 {
        for y in 0..n as i64 {
            for z in 0..n as i64 {
                let c = [x, y, z];
                if !filled(c) {
                    continue;
                }
                for k in 0..3 {
                    for dir in [-1, 1] {
                        let mut o = c;
                        o[k] += dir;
                        if filled(o) {
                            continue;
                        }
                        // The side's corners counter-clockwise about its
                        // outward normal `dir·e_k`.
                        let (i, j) = ((k + 1) % 3, (k + 2) % 3);
                        let at = |a: i64, b: i64| {
                            let mut p = c;
                            p[k] += i64::from(dir > 0);
                            p[i] += a;
                            p[j] += b;
                            p
                        };
                        let ring = if dir > 0 {
                            [at(0, 0), at(1, 0), at(1, 1), at(0, 1)]
                        } else {
                            [at(0, 0), at(0, 1), at(1, 1), at(1, 0)]
                        };
                        quads.push(ring.map(&mut vert));
                    }
                }
            }
        }
    }
    Tris::quads(verts, &quads)
}

fn cube() -> Tris {
    cells(1, 10.0, |_| true)
}

/// The Menger sponge of `level` (1 or 2), 27 mm across.
fn sponge(level: u32) -> Tris {
    let n = 3usize.pow(level);
    let s = 27.0 / n as f64;
    cells(n, s, |c| {
        let mut m = c;
        (0..level).all(|_| {
            let ones = m.iter().filter(|&&x| x % 3 == 1).count();
            m = m.map(|x| x / 3);
            ones < 2
        })
    })
}

/// A staircase of `steps` steps in `x`, each 1 mm deep and high, 2 mm
/// wide: concave creases between the treads and risers.
fn stairs(steps: usize) -> Tris {
    cells(steps, 1.0, |[x, _, z]| z <= x)
}

fn tetrahedron() -> Tris {
    Tris {
        verts: vec![
            [-1.0, -1.0, 1.0],
            [-1.0, 1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, 1.0],
        ],
        tris: vec![[2, 0, 1], [0, 3, 1], [2, 3, 0], [1, 3, 2]],
    }
}

fn octahedron() -> Tris {
    let verts = vec![
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let tris = vec![
        [0, 2, 4],
        [2, 1, 4],
        [1, 3, 4],
        [3, 0, 4],
        [2, 0, 5],
        [1, 2, 5],
        [3, 1, 5],
        [0, 3, 5],
    ];
    Tris { verts, tris }
}

/// A prism on the regular `n`-gon of radius 5 (corners rounded), height
/// 7: caps as fans from their first corner, sides as split quads. Each
/// side's four corners lie on one vertical plane exactly.
fn prism(n: usize) -> Tris {
    let mut verts = Vec::new();
    for z in [0.0, 7.0] {
        for k in 0..n {
            let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            verts.push([5.0 * trig::cos(a), 5.0 * trig::sin(a), z]);
        }
    }
    let mut tris = Vec::new();
    for k in 1..n - 1 {
        tris.push([0, k + 1, k]);
        tris.push([n, n + k, n + k + 1]);
    }
    let mut t = Tris { verts, tris };
    for k in 0..n {
        let j = (k + 1) % n;
        t.tris.push([k, j, n + j]);
        t.tris.push([k, n + j, n + k]);
    }
    t
}

/// The icosahedron of radius about 10, each triangle cut in four with
/// the midpoints pushed out to the sphere (rounded): no two triangles
/// on one plane.
fn icosphere() -> Tris {
    let g = (1.0 + 5f64.sqrt()) / 2.0;
    let mut verts: Vec<[f64; 3]> = [
        [-1.0, g, 0.0],
        [1.0, g, 0.0],
        [-1.0, -g, 0.0],
        [1.0, -g, 0.0],
        [0.0, -1.0, g],
        [0.0, 1.0, g],
        [0.0, -1.0, -g],
        [0.0, 1.0, -g],
        [g, 0.0, -1.0],
        [g, 0.0, 1.0],
        [-g, 0.0, -1.0],
        [-g, 0.0, 1.0],
    ]
    .to_vec();
    let base: [[usize; 3]; 20] = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let r = 10.0 / (1.0 + g * g).sqrt();
    for p in &mut verts {
        *p = p.map(|x| x * r);
    }
    let mut mid = BTreeMap::new();
    let mut tris = Vec::new();
    for [a, b, c] in base {
        let mut m = |i: usize, j: usize| -> usize {
            *mid.entry((i.min(j), i.max(j))).or_insert_with(|| {
                let p: [f64; 3] = [0, 1, 2].map(|k| verts[i][k] + verts[j][k]);
                let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                verts.push(p.map(|x| x * 10.0 / l));
                verts.len() - 1
            })
        };
        let (ab, bc, ca) = (m(a, b), m(b, c), m(c, a));
        tris.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
    }
    Tris { verts, tris }
}

/// The counts of faces, edges, vertices and of loops beyond one a face.
fn counts(b: &Brep) -> [usize; 4] {
    [
        b.face_count(),
        b.edge_count(),
        b.vert_count(),
        b.loop_count() - b.face_count(),
    ]
}

/// The check passes but for the parts this build doesn't certify yet,
/// which come after every edge's tube: a vertex's ball (faces meeting
/// there alone) and faces apart (9); with 9 left out, it passes.
#[track_caller]
fn passes_check(b: &Brep) {
    let budget = Budget::DEFAULT;
    let past = [EmbedCheck::Vertex as u32, EmbedCheck::Apart as u32];
    match b.check(&budget) {
        Ok(()) => {}
        Err(CheckError::Failed(f))
            if f.inv == Inv::Embedding
                && f.finding == Finding::Unbuilt
                && past.contains(&f.check) => {}
        Err(e) => panic!("check: {e:?}"),
    }
    b.check_except(Inv::Embedding, &budget)
        .unwrap_or_else(|e| panic!("check without 9: {e:?}"));
}

/// The body's certified volume holds the mesh's exact one, its error
/// within `rel` of it.
#[track_caller]
fn volume_exact(b: &Brep, mesh: &Tris, rel: f64) {
    let exact = nearest(&mesh.volume());
    let v = mass_props(b, &Budget::DEFAULT)
        .expect("mass properties")
        .volume;
    let scale = exact.abs();
    assert!(
        (v.v - exact).abs() <= v.err() + 1e-15 * scale,
        "{v:?} misses {exact}"
    );
    assert!(v.err() <= rel * scale, "{v:?}: error above {rel}");
}

/// The body of `mesh` has the faces, edges, vertices and holes `counts`,
/// passes its check and holds its volume: to 1e-12 where every vertex's
/// UV on its plane is exact (planes along the axes, coordinates of few
/// bits), else to the ribbons of the UV's rounding on the plane's cell
/// (`2²²` mm: a vertex up to `2⁻³²` mm off its stored plane).
#[track_caller]
fn round_trips(name: &str, mesh: &Tris, want: [usize; 4], rel: f64) {
    let b = mesh.body();
    assert_eq!(counts(&b), want, "{name}");
    passes_check(&b);
    volume_exact(&b, mesh, rel);
}

/// The corpus: each mesh's faces merged by plane (the counts), its check
/// and its volume.
#[test]
fn the_corpus_round_trips() {
    for (name, mesh, want, rel) in [
        ("cube", cube(), [6, 12, 8, 0], 1e-12),
        ("tetrahedron", tetrahedron(), [4, 6, 4, 0], 1e-8),
        ("octahedron", octahedron(), [8, 12, 6, 0], 1e-8),
        // Six sides with a hole each and 24 tunnel walls; the cube's
        // edges cut at the grid's points on them.
        ("sponge", sponge(1), [30, 96, 64, 6], 1e-12),
        ("prism", prism(16), [18, 48, 32, 0], 1e-8),
        ("stairs", stairs(3), [10, 48, 40, 0], 1e-12),
        ("icosphere", icosphere(), [80, 120, 42, 0], 1e-8),
    ] {
        round_trips(name, &mesh, want, rel);
    }
}

/// A level-2 sponge (8640 triangles, 1296 edges once merged) is checked
/// within the budget: the faces across each edge certified on their
/// planes' one normal each.
#[test]
fn a_level_2_sponge_round_trips() {
    round_trips("sponge 2", &sponge(2), [414, 1296, 800, 78], 1e-12);
}

/// Faces merge by exact coplanarity, never by distance: a cube's corner
/// moved off its side's plane by an ulp leaves that side's quad unflat,
/// so it stays two faces, joined at a crease the check refuses (faces a
/// rounding apart along a whole edge: no tube fits), not merged.
#[test]
fn faces_a_rounding_apart_stay_apart() {
    let mut mesh = cube();
    let corner = mesh
        .verts
        .iter()
        .position(|p| *p == [10.0; 3])
        .expect("the far corner");
    mesh.verts[corner][0] = 10.0f64.next_up();
    let b = mesh.body();
    assert_eq!(b.face_count(), 7);
    let err = b.check(&Budget::DEFAULT).expect_err("an unflat crease");
    assert_eq!(err.inv(), Some(Inv::Embedding), "{err:?}");
    let r = from_polyhedron(
        &mesh.verts,
        &mesh.tris,
        ctx(),
        Tolerance::DEFAULT,
        &Budget::DEFAULT,
    );
    assert!(matches!(r, Err(PolyhedronError::Kernel(_))), "{r:?}");
}

/// Meshes that make no closed surface, each refused with its reason.
#[test]
fn bad_meshes_are_refused() {
    let refused = |mesh: &Tris| mesh.build().expect_err("refused");
    let mut m = cube();
    m.tris.pop();
    assert!(matches!(refused(&m), PolyhedronError::Open { .. }));
    let mut m = cube();
    m.tris[0] = [m.tris[0][0], m.tris[0][2], m.tris[0][1]];
    assert!(matches!(refused(&m), PolyhedronError::Open { .. }));
    let mut m = cube();
    m.tris[3][1] = m.verts.len();
    assert_eq!(refused(&m), PolyhedronError::Index { tri: 3 });
    let mut m = cube();
    let [a, b, _] = m.tris[5];
    m.tris[5] = [a, b, a];
    assert_eq!(refused(&m), PolyhedronError::Degenerate { tri: 5 });
    let mut m = cube();
    m.verts.push([-0.0, 0.0, 0.0]);
    let i = m
        .verts
        .iter()
        .position(|p| *p == [0.0; 3])
        .expect("a corner");
    assert_eq!(
        refused(&m),
        PolyhedronError::Duplicate {
            verts: [i, m.verts.len() - 1]
        }
    );
    // Two cubes apart.
    let mut m = cube();
    let n = m.verts.len();
    let other = cube();
    m.verts
        .extend(other.verts.iter().map(|p| [p[0] + 20.0, p[1], p[2]]));
    m.tris.extend(other.tris.iter().map(|t| t.map(|i| i + n)));
    assert_eq!(refused(&m), PolyhedronError::Pieces);
    let mut m = cube();
    let far = [1e300, 0.0, 0.0];
    m.verts.push(far);
    assert!(matches!(refused(&m), PolyhedronError::Kernel(_)));
}

/// A knife edge: triangles across an edge on one plane but facing
/// opposite ways (a flat tetrahedron, its fourth corner on its base's
/// plane beyond an edge) are not merged, and refused.
#[test]
fn a_knife_edge_is_refused() {
    let m = Tris {
        verts: vec![
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [0.0, 4.0, 0.0],
            [5.0, 5.0, 0.0],
        ],
        tris: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]],
    };
    assert_eq!(
        m.build().expect_err("a knife"),
        PolyhedronError::Knife { edge: [0, 1] }
    );
}
