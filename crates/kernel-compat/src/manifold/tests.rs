use super::*;

/// The tetrahedron on the origin and the three unit axes, facing out.
fn tetrahedron() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let positions = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let triangles = vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
    (positions, triangles)
}

#[test]
fn collinear_is_decided_exactly() {
    let p = |x: f32, y: f32| DVec3::new(f64::from(x), f64::from(y), 0.0);
    // On a line, with coordinates far apart in size.
    let t = 1e-20f32;
    assert!(cross_component_is_zero(
        [p(0.0, 0.0), p(1e6, 1.0), p(2e6, 2.0)],
        2
    ));
    assert!(cross_component_is_zero(
        [p(t, t), p(1.0, 1.0), p(2e6, 2e6)],
        2
    ));
    // Off the line by `t`, which the differences in `f64` round away:
    // `(p1 − p0) × (p2 − p0)` is `−2t + t = −t` exactly, but `0` from the
    // rounded differences.
    let tri = [p(t, 0.0), p(1e6, 1.0), p(2e6, 2.0)];
    let (u, v) = (tri[1] - tri[0], tri[2] - tri[0]);
    assert_eq!(u.x * v.y - u.y * v.x, 0.0);
    assert!(!cross_component_is_zero(tri, 2));
    // The other components of a triangle in `z = 0` are zero outright.
    assert!(cross_component_is_zero(tri, 0));
    assert!(cross_component_is_zero(tri, 1));
}

#[test]
fn the_expansion_sums_exactly() {
    let mut e = Vec::new();
    for b in [1e300, 1.0, -1e300, 1e-300, -1.0, -1e-300] {
        grow(&mut e, b);
    }
    assert!(e.iter().all(|&x| x == 0.0), "{e:?}");
    let mut e = Vec::new();
    for b in [1e16, 1.0, -1e16] {
        grow(&mut e, b);
    }
    assert_eq!(e.iter().sum::<f64>(), 1.0);
}

#[test]
fn a_tetrahedron_passes() {
    let (p, t) = tetrahedron();
    let mesh = ManifoldMesh::new([0.0; 3], p.clone(), t.clone()).unwrap();
    assert_eq!(mesh.volume(), 1.0 / 6.0);
    let moved = ManifoldMesh::new([-3.0, 1e6, 2e6], p.clone(), t.clone()).unwrap();
    assert_eq!(moved.origin(), [-3.0, 1e6, 2e6]);
    for origin in [[0.5, 0.0, 0.0], [0.0, 2e6 + 1.0, 0.0], [f64::NAN, 0.0, 0.0]] {
        assert_eq!(
            ManifoldMesh::new(origin, p.clone(), t.clone()),
            Err(ManifoldError::Origin)
        );
    }
}

/// A change to a tetrahedron's parts.
type Edit = dyn Fn(&mut Vec<[f64; 3]>, &mut Vec<[u32; 3]>);

#[test]
fn broken_parts_are_refused() {
    let refused = |edit: &Edit| {
        let (mut p, mut t) = tetrahedron();
        edit(&mut p, &mut t);
        ManifoldMesh::new([0.0; 3], p, t).unwrap_err()
    };
    assert_eq!(refused(&|_, t| t.clear()), ManifoldError::Empty);
    assert_eq!(
        refused(&|p, _| p[2][1] = f64::NAN),
        ManifoldError::Position(2)
    );
    assert_eq!(refused(&|p, _| p[3][2] = 3e6), ManifoldError::Position(3));
    // Not an `f32`: a reader keeping single precision would move it.
    assert_eq!(refused(&|p, _| p[1][0] = 0.1), ManifoldError::Position(1));
    assert_eq!(refused(&|_, t| t[1][2] = 4), ManifoldError::Index(1));
    assert_eq!(
        refused(&|_, t| t[2] = [0, 3, 0]),
        ManifoldError::RepeatedVertex(2)
    );
    // Vertex 3 on the line through 0 and 1: triangle 1 is flat.
    assert_eq!(
        refused(&|p, _| p[3] = [0.5, 0.0, 0.0]),
        ManifoldError::Degenerate(1)
    );
    // Exactly on the line, though far from any round number.
    assert_eq!(
        refused(&|p, _| {
            let tiny = f64::from(1e-38f32);
            p[1] = [3.0, tiny, 7.0];
            p[3] = [6.0, 2.0 * tiny, 14.0];
        }),
        ManifoldError::Degenerate(1)
    );
    // A copy of vertex 0 standing in for it in one triangle.
    assert_eq!(
        refused(&|p, t| {
            p.push([-0.0, 0.0, 0.0]);
            t[3] = [1, 2, 4];
        }),
        ManifoldError::Coincident(0, 4)
    );
    assert_eq!(
        refused(&|_, t| {
            t.pop();
        }),
        ManifoldError::EdgeUse {
            a: 1,
            b: 2,
            uses: 1
        }
    );
    // A second copy of a triangle: its edges are used three times.
    assert_eq!(
        refused(&|_, t| t.push([1, 2, 3])),
        ManifoldError::EdgeUse {
            a: 1,
            b: 2,
            uses: 3
        }
    );
    assert_eq!(
        refused(&|_, t| t[0] = [0, 1, 2]),
        ManifoldError::Orientation { a: 0, b: 1 }
    );
    assert_eq!(
        refused(&|p, _| p.push([5.0; 3])),
        ManifoldError::UnusedVertex(4)
    );
    assert_eq!(
        refused(&|p, t| {
            p.insert(0, [5.0; 3]);
            for tri in t.iter_mut() {
                *tri = tri.map(|v| v + 1);
            }
        }),
        ManifoldError::UnusedVertex(0)
    );
    // Two tetrahedra sharing a corner: a closed surface pinched there.
    assert_eq!(
        refused(&|p, t| {
            p.extend([[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]);
            t.extend([[0, 4, 5], [0, 6, 4], [0, 5, 6], [4, 6, 5]]);
        }),
        ManifoldError::Fan(0)
    );
    assert_eq!(
        refused(&|_, t| {
            for tri in t.iter_mut() {
                tri.swap(1, 2);
            }
        }),
        ManifoldError::InsideOut
    );
}

/// `x` rounded to a multiple of `2^-20`: an `f32`, as a position must
/// be, below 16, and one the plain check can tell.
fn on_grid(x: f64) -> f64 {
    let grid = f64::from(1u32 << 20);
    (x * grid).round() / grid
}

/// A box from `min` to `max`: its eight corners, two triangles a side.
fn cuboid(min: [f64; 3], max: [f64; 3]) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let p = (0..8)
        .map(|c| [0, 1, 2].map(|i| if c >> i & 1 == 0 { min[i] } else { max[i] }))
        .collect();
    let mut t = Vec::new();
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for side in 0..2 {
            let mut ring =
                [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(a, b)| side << axis | a << u | b << v);
            if side == 0 {
                ring.reverse();
            }
            t.extend([[ring[0], ring[1], ring[2]], [ring[0], ring[2], ring[3]]]);
        }
    }
    (p, t)
}

/// A cylinder of `radius` and `height` on `base` up Z, faceted in `sides`,
/// its caps fanned from their centres.
fn cylinder(
    base: [f64; 3],
    radius: f64,
    height: f64,
    sides: u32,
) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let mut p = vec![base, [base[0], base[1], base[2] + height]];
    for z in [0.0, height] {
        for i in 0..sides {
            let angle = std::f64::consts::TAU * f64::from(i) / f64::from(sides);
            let (sin, cos) = crate::trig::sin_cos(angle);
            p.push([
                on_grid(base[0] + radius * cos),
                on_grid(base[1] + radius * sin),
                base[2] + z,
            ]);
        }
    }
    let (bottom, top) = (2, 2 + sides);
    let mut t = Vec::new();
    for i in 0..sides {
        let j = (i + 1) % sides;
        t.push([0, bottom + j, bottom + i]);
        t.push([1, top + i, top + j]);
        t.push([bottom + i, bottom + j, top + j]);
        t.push([bottom + i, top + j, top + i]);
    }
    (p, t)
}

/// A torus of radii `major` and `minor` around Z, `around` quads round
/// the axis by `tube` round the tube.
fn torus(around: u32, tube: u32, major: f64, minor: f64) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let mut p = Vec::new();
    for i in 0..around {
        let (su, cu) =
            crate::trig::sin_cos(std::f64::consts::TAU * f64::from(i) / f64::from(around));
        for j in 0..tube {
            let (sv, cv) =
                crate::trig::sin_cos(std::f64::consts::TAU * f64::from(j) / f64::from(tube));
            let r = major + minor * cv;
            p.push([on_grid(r * cu), on_grid(r * su), on_grid(minor * sv)]);
        }
    }
    let at = |i: u32, j: u32| (i % around) * tube + j % tube;
    let mut t = Vec::new();
    for i in 0..around {
        for j in 0..tube {
            let [a, b, c, d] = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            t.extend([[a, b, c], [a, c, d]]);
        }
    }
    (p, t)
}

#[test]
fn hand_built_shells_pass() {
    let shells = [
        cuboid([-1.0; 3], [2.0, 3.0, 4.0]),
        cylinder([1.0, 2.0, 3.0], 1.0, 2.0, 16),
        torus(8, 6, 3.0, 1.0),
    ];
    for (p, t) in shells {
        assert_eq!(plainly_manifold([0.0; 3], &p, &t), Some(true));
        assert!(ManifoldMesh::new([0.0; 3], p, t).is_ok());
    }
}

/// Whether these parts make a [`ManifoldMesh`], checked the plain way,
/// independently of [`ManifoldMesh::new`]: maps of edges and corners, and
/// collinearity in integers. `None` where that can't tell: a coordinate
/// that isn't a multiple of `2^-40` below `2^20`, or a volume too near
/// zero to tell its sign in `f64`.
fn plainly_manifold(origin: [f64; 3], p: &[[f64; 3]], t: &[[u32; 3]]) -> Option<bool> {
    use std::collections::{BTreeMap, BTreeSet};
    let whole = |x: f64| x.abs() <= ManifoldMesh::MAX_POSITION && x.fract() == 0.0;
    if t.is_empty() || !origin.iter().all(|&x| whole(x)) {
        return Some(false);
    }
    let fits = |x: f64| x.is_finite() && x.abs() <= ManifoldMesh::MAX_POSITION;
    if !p
        .iter()
        .flatten()
        .all(|&x| fits(x) && f64::from(x as f32) == x)
    {
        return Some(false);
    }
    let fixed = |x: f64| {
        let scaled = x * (1u64 << 40) as f64;
        (scaled.fract() == 0.0 && scaled.abs() < (1u64 << 60) as f64).then_some(scaled as i128)
    };
    let q: Vec<[i128; 3]> = (p.iter())
        .map(|v| Some([fixed(v[0])?, fixed(v[1])?, fixed(v[2])?]))
        .collect::<Option<_>>()?;
    if (q.iter().collect::<BTreeSet<_>>()).len() != q.len() {
        return Some(false);
    }
    let mut directed = BTreeMap::new();
    let mut around: BTreeMap<u32, BTreeMap<u32, u32>> = BTreeMap::new();
    for &[a, b, c] in t {
        if [a, b, c].iter().any(|&v| v as usize >= p.len()) || a == b || b == c || c == a {
            return Some(false);
        }
        let [pa, pb, pc] = [a, b, c].map(|v| q[v as usize]);
        let u = [0, 1, 2].map(|i| pb[i] - pa[i]);
        let w = [0, 1, 2].map(|i| pc[i] - pa[i]);
        let cross = [0, 1, 2].map(|i| {
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            u[j] * w[k] - u[k] * w[j]
        });
        if cross == [0; 3] {
            return Some(false);
        }
        for (from, to, next) in [(a, b, c), (b, c, a), (c, a, b)] {
            if directed.insert((from, to), ()).is_some() {
                return Some(false);
            }
            around.entry(from).or_default().insert(to, next);
        }
    }
    if directed
        .keys()
        .any(|&(a, b)| !directed.contains_key(&(b, a)))
    {
        return Some(false);
    }
    if around.len() != p.len() {
        return Some(false);
    }
    for links in around.values() {
        let (&start, _) = links.iter().next().unwrap();
        let mut at = start;
        let mut seen = 0;
        loop {
            at = links[&at];
            seen += 1;
            if at == start {
                break;
            }
        }
        if seen != links.len() {
            return Some(false);
        }
    }
    let volume = signed_volume(p, t);
    let scale: f64 = (t.iter())
        .map(|tri| {
            let [a, b, c] = tri.map(|v| DVec3::from_array(p[v as usize]));
            a.length() * b.length() * c.length()
        })
        .sum();
    if volume.abs() <= 1e-9 * scale {
        return None;
    }
    Some(volume > 0.0)
}

#[test]
fn mutated_meshes_are_refused_unless_still_manifolds() {
    use varde_testkit::rng::Rng;
    let meshes: Vec<ManifoldMesh> = [
        cuboid([-1.0; 3], [2.0, 3.0, 4.0]),
        cylinder([1.0, 2.0, 3.0], 1.0, 2.0, 16),
        torus(8, 6, 3.0, 1.0),
    ]
    .into_iter()
    .map(|(p, t)| ManifoldMesh::new([0.0; 3], p, t).unwrap())
    .collect();
    let mut rng = Rng::new(60);
    let (mut kept, mut refused, mut untold) = (0, 0, 0);
    // Quick mode runs the first quarter.
    let cases = varde_testing::pick(1500, 6000);
    for case in 0..cases {
        let mesh = &meshes[case % meshes.len()];
        let mut origin = mesh.origin();
        let mut p = mesh.positions().to_vec();
        let mut t = mesh.triangles().to_vec();
        let pick = |rng: &mut Rng, n: usize| (rng.next_u64() % n as u64) as usize;
        for _ in 0..1 + pick(&mut rng, 3) {
            let (nt, np) = (t.len().max(1), p.len().max(1));
            match pick(&mut rng, 17) {
                0 => t
                    .get_mut(pick(&mut rng, nt))
                    .map_or((), |tri| tri.swap(0, 1)),
                1 => {
                    let v = pick(&mut rng, np + 1) as u32;
                    t.get_mut(pick(&mut rng, nt))
                        .map_or((), |tri| tri[pick(&mut rng, 3)] = v)
                }
                2 => {
                    if !t.is_empty() {
                        t.remove(pick(&mut rng, nt));
                    }
                }
                3 => t.extend(t.get(pick(&mut rng, nt)).copied()),
                4 => t.extend(t.get(pick(&mut rng, nt)).map(|&[a, b, c]| [a, c, b])),
                5 => {
                    let from = p[pick(&mut rng, np)];
                    p[pick(&mut rng, np)] = from;
                }
                6 => p.push([7.5, -2.25, 0.125]),
                // Two vertices welded into one.
                7 => {
                    let (keep, gone) = (pick(&mut rng, np) as u32, pick(&mut rng, np) as u32);
                    for tri in &mut t {
                        *tri = tri.map(|v| if v == gone { keep } else { v });
                    }
                }
                // A vertex split off for one triangle, a hair away.
                8 => {
                    let i = pick(&mut rng, nt);
                    let corner = pick(&mut rng, 3);
                    let tri = t.get_mut(i).filter(|tri| (tri[corner] as usize) < p.len());
                    if let Some(tri) = tri {
                        let mut at = p[tri[corner] as usize];
                        at[0] = f64::from(f32::from_bits((at[0] as f32).to_bits() + 1));
                        tri[corner] = p.len() as u32;
                        p.push(at);
                    }
                }
                9 => t.iter_mut().for_each(|tri| tri.swap(1, 2)),
                // Renumbered and reordered: still a manifold.
                10 => {
                    let n = p.len() as u32;
                    let shift = 1 + pick(&mut rng, np) as u32;
                    let renumber = |v: u32| (v + shift) % n;
                    let mut moved = p.clone();
                    for (v, at) in p.iter().enumerate() {
                        moved[renumber(v as u32) as usize] = *at;
                    }
                    p = moved;
                    t = t.iter().map(|tri| tri.map(renumber)).collect();
                    t.reverse();
                    t.iter_mut().for_each(|tri| tri.rotate_left(1));
                }
                // A second copy beside it: two shells, still a manifold;
                // or touching it at a vertex: pinched there.
                11 | 12 => {
                    let n = p.len() as u32;
                    p.extend(p.clone().iter().map(|&[x, y, z]| [x + 64.0, y, z]));
                    let copy: Vec<[u32; 3]> = t.iter().map(|tri| tri.map(|v| v + n)).collect();
                    t.extend(copy);
                    if pick(&mut rng, 2) == 0 {
                        let (keep, gone) =
                            (pick(&mut rng, np) as u32, n + pick(&mut rng, np) as u32);
                        for tri in &mut t {
                            *tri = tri.map(|v| if v == gone { keep } else { v });
                        }
                    }
                }
                13 => {
                    p[pick(&mut rng, np)][pick(&mut rng, 3)] =
                        [f64::NAN, 1e300, 0.1][pick(&mut rng, 3)]
                }
                14 => origin[pick(&mut rng, 3)] += [0.5, 3e6, 1.0][pick(&mut rng, 3)],
                // A vertex moved onto the line through two of its
                // triangle's others, or between them.
                15 => {
                    let tri = t.get(pick(&mut rng, nt));
                    let tri = tri.filter(|tri| tri.iter().all(|&v| (v as usize) < p.len()));
                    if let Some(&[a, b, c]) = tri {
                        let [pa, pb] = [a, b].map(|v| DVec3::from_array(p[v as usize]));
                        let k = [2.0, -1.0, 0.5][pick(&mut rng, 3)];
                        let on = pa + (pb - pa) * k;
                        if on.to_array().iter().all(|&x| f64::from(x as f32) == x) {
                            p[c as usize] = on.to_array();
                        }
                    }
                }
                _ => t.truncate(t.len() / 2),
            }
        }
        let plain = plainly_manifold(origin, &p, &t);
        let checked = ManifoldMesh::new(origin, p.clone(), t.clone());
        match plain {
            Some(true) => {
                assert!(checked.is_ok(), "case {case}: {checked:?}");
                kept += 1;
            }
            Some(false) => {
                assert!(checked.is_err(), "case {case}: taken");
                refused += 1;
            }
            None => untold += 1,
        }
    }
    // Both kinds seen plenty.
    assert!(
        kept * 40 > cases && refused * 2 > cases && untold * 3 < cases,
        "{kept} kept, {refused} refused, {untold} untold"
    );
}
