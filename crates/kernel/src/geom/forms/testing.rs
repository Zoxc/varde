//! What the forms' tests share: exact nets, the rounding check, shared
//! rows and the angle between a tile's normals and the form's.

use malachite::Rational as Q;
use varde_decide::bezier::HomPatch;

use crate::geom::lattice::{Lattice, Level, Rect};
use crate::geom::round::{nearest, q};
use crate::geom::tile::Tile;

/// The tile's exact homogeneous patch.
pub(super) fn exact(t: &Tile) -> HomPatch<Q> {
    t.patch_at(&Q::from(0u32))
}

/// The lattice's frame, exactly: its origin and axes.
pub(super) fn frame(l: &Lattice) -> ([Q; 3], [[Q; 3]; 3]) {
    (l.origin().map(q), l.axes().map(|a| a.map(q)))
}

pub(super) fn dot(a: &[Q; 3], b: &[Q; 3]) -> Q {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

/// The largest coordinate magnitude of a tile's points.
pub(super) fn scale(t: &Tile) -> f64 {
    t.net()
        .iter()
        .flat_map(|p| p[..3].iter().map(|x| x.abs()))
        .fold(0.0, |m, x| if x > m { x } else { m })
}

/// Asserts that `t`'s net is `want`'s exact points rounded once, bit for
/// bit, with its weights.
pub(super) fn rounded_once(t: &Tile, want: &[([Q; 3], f64); 9], what: &str) {
    for (n, (p, w)) in want.iter().enumerate() {
        let got = t.net()[n];
        let p = p.each_ref().map(nearest);
        assert_eq!(
            [got[0], got[1], got[2], got[3]].map(f64::to_bits),
            [p[0], p[1], p[2], *w].map(f64::to_bits),
            "{what}: control point {n}"
        );
    }
}

/// Asserts that every tile of `rect` meets its neighbours in `u` and `v`
/// on a row that is the same bits in both.
pub(super) fn rows_shared(tiles: &[Tile], rect: &Rect) {
    let nv = (rect.v.end - rect.v.start) as usize;
    let nu = (rect.u.end - rect.u.start) as usize;
    let bits = |t: &Tile, n: usize| t.net()[n].map(f64::to_bits);
    for a in 0..nu {
        for b in 0..nv {
            let t = &tiles[a * nv + b];
            if a + 1 < nu {
                let s = &tiles[(a + 1) * nv + b];
                for r in 0..3 {
                    assert_eq!(bits(t, 6 + r), bits(s, r), "u row at ({a}, {b})");
                }
            }
            if b + 1 < nv {
                let s = &tiles[a * nv + b + 1];
                for r in 0..3 {
                    assert_eq!(bits(t, 3 * r + 2), bits(s, 3 * r), "v row at ({a}, {b})");
                }
            }
        }
    }
}

/// The local parameter of index `i` at `levels` below its base cell:
/// `(i mod 2^L)/2^L`.
pub(super) fn local(i: i64, levels: u32) -> Q {
    let m = 1i64 << levels;
    Q::from(i.rem_euclid(m)) / Q::from(m)
}

/// A random level up to `cap` in each direction.
pub(super) fn level_to(cap: Level, rng: &mut varde_testkit::rng::Rng) -> Level {
    Level {
        u: (rng.next_u64() % (u64::from(cap.u) + 1)) as u32,
        v: (rng.next_u64() % (u64::from(cap.v) + 1)) as u32,
    }
}

/// The largest angle (rad) between the coefficients of `t`'s exact
/// normal numerator and the form's normal `out`, asserting each has a
/// positive component along it.
pub(super) fn normal_angle(t: &Tile, out: [f64; 3], what: &str) -> f64 {
    let e = exact(t);
    let [x, y, z] = e.normal_numerator().unwrap().map(|f| f.coefs().to_vec());
    let o = out.map(q);
    let mut worst: f64 = 0.0;
    for m in 0..x.len() {
        let c = [x[m].clone(), y[m].clone(), z[m].clone()];
        let d = dot(&c, &o);
        assert!(d > 0u32, "{what}: coefficient {m} faces in");
        let cos2 = nearest(&(&d * &d / (dot(&c, &c) * dot(&o, &o))));
        let cos = if cos2 < 1.0 { cos2.sqrt() } else { 1.0 };
        let a = varde_decide::num::trig::acos(cos);
        worst = if a > worst { a } else { worst };
    }
    worst
}
