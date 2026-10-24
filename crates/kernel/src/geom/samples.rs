//! Surfaces of every form for the tests of what reads them whole (the
//! per-tile implicits, the inverse): random keys about points up to
//! `1e3` out, each form's canonical surface, and the same rectangle two
//! levels finer.

use varde_testkit::rng::Rng;

use super::conic::Conic;
use super::form::{
    AffineKey, ConeKey, ConicCylinderKey, CylinderKey, FormKey, PlaneKey, RevolvedKey, Span2,
    SphereKey, TorusKey,
};
use super::lattice::{Lattice, Level, Rect};
use super::surface::Surface;
use super::tile::Tile;

/// The `(ρ, z)` axes of a meridian plane.
const MERIDIAN: [[f64; 2]; 2] = [[1.0, 0.0], [0.0, 1.0]];

/// A named surface.
pub(crate) struct Sample {
    pub(crate) name: String,
    pub(crate) surface: Surface,
}

/// The canonical surface of `key` (at angle cell `first`).
fn canonical(key: FormKey, first: i64) -> Surface {
    let lattice = Lattice::new(key).unwrap();
    let rect = lattice.half(first).unwrap();
    Surface::new(lattice, rect).unwrap()
}

/// `s`'s rectangle two levels finer in each direction, capped.
pub(crate) fn refined(s: &Surface) -> Surface {
    let r = s.rect();
    let fine = Rect {
        level: Level {
            u: r.level.u + 2,
            v: r.level.v + 2,
        },
        u: (r.u.start << 2)..(r.u.end << 2),
        v: (r.v.start << 2)..(r.v.end << 2),
    };
    let lattice = s.lattice().clone();
    let rect = lattice.capped(&fine).unwrap();
    Surface::new(lattice, rect).unwrap()
}

/// Tile `(i, j)` of `level` on `lattice`, coarsened to its cell's cap
/// where it is beyond it (beside a singular row, deep in both
/// directions; [`Lattice::capped`]): the level, the indices and the
/// tile at them.
pub(crate) fn capped_tile(
    lattice: &Lattice,
    level: Level,
    i: i64,
    j: i64,
) -> (Level, i64, i64, Tile) {
    let one = Rect {
        level,
        u: i..i + 1,
        v: j..j + 1,
    };
    let c = lattice.capped(&one).unwrap();
    let tile = lattice.tile(c.level, c.u.start, c.v.start).unwrap();
    (c.level, c.u.start, c.v.start, tile)
}

/// Revolved profiles: three quarters of an ellipse off the axis,
/// hyperbola arcs whose joints' given weights differ, a parabola's arcs, an ellipse from pole to pole, an oblique span from
/// an apex, and two straight spans (a cone's and a plane's meridian).
fn profiles(rng: &mut Rng) -> Vec<(&'static str, Vec<Span2>)> {
    let r = rng.log_range(1e-1, 1e2);
    let c = [r * rng.range(1.5, 4.0), r * rng.range(-3.0, 3.0)];
    let e = Conic::ellipse(c, MERIDIAN, [r, r * rng.range(0.3, 1.0)]).unwrap();
    let p = Conic::parabola(c, [[0.0, 1.0], [1.0, 0.0]], r).unwrap();
    // Hyperbola arcs whose given weights differ at their joints.
    let hyp = Conic::hyperbola(c, MERIDIAN, [r, 0.5 * r]).unwrap();
    let z = c[1];
    let poles = Conic::ellipse([0.0, z], MERIDIAN, [r, 1.5 * r]).unwrap();
    // The straight spans' values exact: a power of two and a multiple.
    let l = f64::from_bits(r.to_bits() & 0xfff0_0000_0000_0000);
    let h = (z / l).round() * l;
    vec![
        (
            "ellipse",
            (0..3).map(|k| e.quarter::<3>(k).unwrap()).collect(),
        ),
        (
            "hyperbola",
            [0.6, 0.8, 1.0, 1.2]
                .windows(2)
                .map(|t| hyp.arc::<3>(t[0], t[1]).unwrap())
                .collect(),
        ),
        (
            "parabola",
            [-1.5, -0.5, 0.25, 1.0]
                .windows(2)
                .map(|t| p.arc::<3>(t[0], t[1]).unwrap())
                .collect(),
        ),
        (
            "poles",
            vec![
                poles.quarter::<3>(3).unwrap(),
                poles.quarter::<3>(0).unwrap(),
            ],
        ),
        (
            "apex",
            vec![[
                [0.0, z, 1.0],
                [r, z + 2.0 * r, 0.75],
                [3.0 * r, z + 2.5 * r, 1.0],
            ]],
        ),
        (
            "oblique line",
            vec![[
                [l, h, 1.0],
                [1.5 * l, h + l, 1.0],
                [2.0 * l, h + 2.0 * l, 1.0],
            ]],
        ),
        (
            "flat line",
            vec![[[l, h, 1.0], [2.0 * l, h, 2.0], [3.0 * l, h, 1.0]]],
        ),
    ]
}

/// Surfaces of every form (module docs), canonical and refined.
pub(crate) fn surfaces(seed: u64) -> Vec<Sample> {
    let mut rng = Rng::new(seed);
    let mut keys: Vec<(String, FormKey)> = Vec::new();
    let p = |rng: &mut Rng| rng.point(1e3).to_array();
    let d = |rng: &mut Rng| rng.direction().to_array();
    let (p0, a0) = (p(&mut rng), d(&mut rng));
    keys.push(("plane".into(), PlaneKey::new(a0, p0).unwrap().into()));
    let (p1, a1, r1) = (p(&mut rng), d(&mut rng), rng.log_range(1e-2, 1e2));
    keys.push((
        "cylinder".into(),
        CylinderKey::new(p1, a1, r1).unwrap().into(),
    ));
    let a2 = d(&mut rng);
    let (e1, e2) = rng.direction().any_orthonormal_pair();
    let c2 = p(&mut rng);
    let r2 = rng.log_range(1e-1, 1e2);
    let ell = Conic::ellipse(c2, [e1.to_array(), e2.to_array()], [r2, 0.6 * r2]).unwrap();
    let spans: Vec<_> = (0..3).map(|k| ell.quarter::<4>(k).unwrap()).collect();
    keys.push((
        "conic chain".into(),
        ConicCylinderKey::new(a2, &spans).unwrap().into(),
    ));
    let hyp = Conic::hyperbola(c2, [e1.to_array(), e2.to_array()], [r2, 0.5 * r2]).unwrap();
    let spans: Vec<_> = [0.6, 0.8, 1.0, 1.2]
        .windows(2)
        .map(|t| hyp.arc::<4>(t[0], t[1]).unwrap())
        .collect();
    keys.push((
        "hyperbola chain".into(),
        ConicCylinderKey::new(a2, &spans).unwrap().into(),
    ));
    let (p3, a3) = (p(&mut rng), d(&mut rng));
    let (along, across) = (rng.range(0.2, 1.0), rng.range(0.2, 1.0));
    keys.push((
        "cone".into(),
        ConeKey::new(p3, a3, along, across).unwrap().into(),
    ));
    let (p4, a4, r4) = (p(&mut rng), d(&mut rng), rng.log_range(1e-2, 1e2));
    keys.push(("sphere".into(), SphereKey::new(p4, r4, a4).unwrap().into()));
    let (p5, a5, r5) = (p(&mut rng), d(&mut rng), rng.log_range(1e-1, 1e2));
    keys.push((
        "ring torus".into(),
        TorusKey::new(p5, a5, r5 * rng.range(1.2, 4.0), r5)
            .unwrap()
            .into(),
    ));
    keys.push((
        "spindle torus".into(),
        TorusKey::new(p5, a5, r5 * rng.range(0.2, 0.8), r5)
            .unwrap()
            .into(),
    ));
    let (p6, a6) = (p(&mut rng), d(&mut rng));
    for (name, profile) in profiles(&mut rng) {
        // About an axis through the origin, the straight spans' stored
        // `z` is their given one, still collinear.
        let at = if name.ends_with("line") { [0.0; 3] } else { p6 };
        keys.push((
            format!("revolved {name}"),
            RevolvedKey::new(at, a6, &profile).unwrap().into(),
        ));
    }
    let general: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| rng.range(-1.0, 1.0) + if r == c { 2.0 } else { 0.0 })
    });
    let shift = p(&mut rng);
    let sources: Vec<(String, FormKey)> = keys
        .iter()
        .filter(|(n, _)| {
            matches!(
                n.as_str(),
                "sphere" | "ring torus" | "conic chain" | "revolved ellipse" | "cone"
            )
        })
        .cloned()
        .collect();
    for (name, key) in sources {
        keys.push((
            format!("affine {name}"),
            AffineKey::new(key, general, shift).unwrap().into(),
        ));
    }
    let mut out = Vec::new();
    for (k, (name, key)) in keys.into_iter().enumerate() {
        let first = [0, 1, -1, 2][k % 4];
        let s = canonical(key, first);
        out.push(Sample {
            name: format!("{name} (refined)"),
            surface: refined(&s),
        });
        out.push(Sample { name, surface: s });
    }
    out
}
